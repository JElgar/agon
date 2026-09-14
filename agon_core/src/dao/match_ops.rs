//! Match operations: create (meta + sides + players in one transaction),
//! get aggregate, update meta, live-scoring score record, and player roster
//! writes.

use std::collections::HashMap;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{
    AttributeValue, DeleteRequest, Put, TransactWriteItem, Update, WriteRequest,
};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, ItemBuilder, from_item, item_pk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{
    ConfirmedScoreRecord, HeaderPhotoRecord, MatchFormatRecord, MatchPlayerRecord, MatchRecord,
    MatchScoreRecord, MatchSideRecord, PendingScoreRecord, SideRosterMemberRecord,
};

pub const TYPE_MATCH: &str = "match";
pub const TYPE_MATCH_SIDE: &str = "match_side";
pub const TYPE_MATCH_PLAYER: &str = "match_player";
pub const TYPE_MATCH_SCORE: &str = "match_score";

/// A side's roster is cached in full (`MatchSideRecord::roster_preview`) only
/// when it's this small or smaller — enough for 1v1 and doubles/small squads.
/// Above the cap the preview is left empty and callers fall back to
/// team name/logo (see the field's doc comment for why a partial peek isn't
/// stored). A feed reading the *stored* cache doesn't need this constant: an
/// empty `roster_preview` already means "show the team instead," by
/// construction. `agon_service` does need it — to make the same "small
/// enough to show players" call when resolving a `Match`'s sides *live* from
/// a full player list (see `Api::resolve_side_names`), so both paths agree.
pub const ROSTER_PREVIEW_CAP: usize = 4;

/// Group `players` by `side.side_id` and compute the `(player_count,
/// roster_preview)` pair to store on that side — the roster only when it fits
/// within [`ROSTER_PREVIEW_CAP`], otherwise empty. Pure/sync; shared by
/// `create_match` (already has the full roster in memory) and
/// `refresh_side_roster_previews` (re-queries it).
///
/// The two halves deliberately count different things. `player_count` is the
/// figure a side's `max_players` is enforced against, so it counts only
/// players who take a spot ([`MatchPlayerRecord::occupies_slot`]) — it used to
/// count every row on the side, so a pending or declined invite held a spot.
/// `roster_preview` is a display of who's been placed on the side, pending
/// invitees included, exactly as before: narrowing it would change what feed
/// cards show, a separate call from getting the count right.
///
/// A waitlisted player is in neither. Their `side_id` is the side they asked
/// for, not one they're on, so they'd be wrong in a side's preview (or, via
/// `Api::resolve_side_names`, as the name of a side they aren't playing for).
fn side_roster(side_id: &str, players: &[MatchPlayerRecord]) -> (u32, Vec<SideRosterMemberRecord>) {
    let on_side: Vec<&MatchPlayerRecord> = players
        .iter()
        .filter(|p| p.side_id.as_deref() == Some(side_id) && p.waitlisted_at.is_none())
        .collect();
    let player_count = on_side.iter().filter(|p| p.occupies_slot()).count() as u32;
    let roster_preview = if on_side.len() <= ROSTER_PREVIEW_CAP {
        on_side
            .into_iter()
            .map(|p| SideRosterMemberRecord {
                player_id: p.player_id.clone(),
                user_id: p.user_id.clone(),
                display_name: p.display_name.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    (player_count, roster_preview)
}

/// The match-wide headcount `MatchRecord::total_player_count` stores: players
/// who take a spot ([`MatchPlayerRecord::occupies_slot`]) across every side
/// plus unassigned — not every roster row.
fn total_player_count(players: &[MatchPlayerRecord]) -> u64 {
    players.iter().filter(|p| p.occupies_slot()).count() as u64
}

/// How many times [`Dao::refresh_side_roster_previews`] re-reads and recounts
/// when a concurrent roster write lands between its read and its guarded
/// write. Each attempt starts from a fresh strongly consistent read, so one
/// retry normally settles it; the bound only matters under sustained
/// contention, where every competing writer brings its own recount anyway.
const ROSTER_RECOUNT_ATTEMPTS: u32 = 5;

/// A match's headcounts as [`Dao::refresh_side_roster_previews`] just
/// recomputed and stored them. Returned so a caller acting on the fresh counts
/// ([`Dao::recount_has_room`], re-checking capacity) needn't read them back — a
/// read that, being eventually consistent, could still see the old ones.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterCounts {
    pub total_player_count: u64,
    /// Keyed by side id.
    pub side_player_counts: HashMap<String, u32>,
}

/// The counters on a match's meta item as read at the start of a recount —
/// the values its write is guarded on. `None` means the attribute is absent
/// (items written before it existed), which the guard has to express as
/// `attribute_not_exists`: `= 0` is false against a missing attribute, so
/// defaulting to zero here would fail the guard on every attempt.
#[derive(Debug, Default, serde::Deserialize)]
struct StoredRosterCounts {
    #[serde(default)]
    total_player_count: Option<u64>,
    #[serde(default)]
    sides: HashMap<String, StoredSideCount>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct StoredSideCount {
    #[serde(default)]
    player_count: Option<u32>,
}

/// A recount's guarded `UpdateItem`, and the counts it stores.
struct RecountWrite {
    update_expression: String,
    condition_expression: String,
    names: HashMap<String, String>,
    values: HashMap<String, AttributeValue>,
    counts: RosterCounts,
}

/// Build the recount write for a match from its counters as `stored` and its
/// players as queried *after* them. Pure, so the guard — the part that stops a
/// recount clobbering a concurrent `ADD` — can be unit-tested directly; see
/// [`Dao::refresh_side_roster_previews`] for why it's shaped this way.
fn recount_write(
    stored: &StoredRosterCounts,
    players: &[MatchPlayerRecord],
) -> DaoResult<RecountWrite> {
    let total = total_player_count(players);
    let mut set = vec!["total_player_count = :tpc".to_string()];
    let mut conditions = vec![
        "attribute_exists(#pk)".to_string(),
        match stored.total_player_count {
            Some(_) => "total_player_count = :read_tpc".to_string(),
            None => "attribute_not_exists(total_player_count)".to_string(),
        },
    ];
    let mut names = HashMap::from([("#pk".to_string(), ATTR_PK.to_string())]);
    let mut values = HashMap::from([(":tpc".to_string(), to_attr(&total)?)]);
    if let Some(read) = stored.total_player_count {
        values.insert(":read_tpc".to_string(), to_attr(&read)?);
    }

    // Sorted only so the expression is deterministic (a map has no stable
    // order), which keeps it testable.
    let mut side_ids: Vec<&String> = stored.sides.keys().collect();
    side_ids.sort();
    let mut side_player_counts = HashMap::with_capacity(side_ids.len());
    for (i, side_id) in side_ids.into_iter().enumerate() {
        let (player_count, roster_preview) = side_roster(side_id, players);
        let side = format!("sides.#s{i}");
        set.push(format!(
            "{side}.player_count = :pc{i}, {side}.roster_preview = :rp{i}"
        ));
        conditions.push(match stored.sides[side_id].player_count {
            Some(read) => {
                values.insert(format!(":read_pc{i}"), to_attr(&read)?);
                format!("{side}.player_count = :read_pc{i}")
            }
            None => format!("attribute_not_exists({side}.player_count)"),
        });
        names.insert(format!("#s{i}"), side_id.clone());
        values.insert(format!(":pc{i}"), to_attr(&player_count)?);
        values.insert(format!(":rp{i}"), to_attr(&roster_preview)?);
        side_player_counts.insert(side_id.clone(), player_count);
    }

    Ok(RecountWrite {
        update_expression: format!("SET {}", set.join(", ")),
        condition_expression: conditions.join(" AND "),
        names,
        values,
        counts: RosterCounts {
            total_player_count: total,
            side_player_counts,
        },
    })
}

/// Extract a match's sides out of their storage map into the stable,
/// deterministic order every caller of [`MatchAggregate`]/[`MatchSummary`]
/// expects (sorted by `side_id`) — a map has no reliable read-back order of
/// its own. Not the *display* order (no "your side first"/"team you know
/// more people on first" — that's a read-time, per-viewer decision for
/// `agon_service` to make); just a stable one so which side renders first
/// doesn't flip between requests.
fn sorted_sides(sides: &HashMap<String, MatchSideRecord>) -> Vec<MatchSideRecord> {
    let mut sides: Vec<MatchSideRecord> = sides.values().cloned().collect();
    sides.sort_by(|a, b| a.side_id.cmp(&b.side_id));
    sides
}

/// The match's overall roster cap, derived from its sides' own caps rather
/// than stored separately: if *every* side has `max_players` set, the sum of
/// them; otherwise uncapped (`None`). The unassigned pool counts toward this
/// but has no cap of its own — enforced by [`take_slot_update`].
pub fn effective_max_players(sides: &[MatchSideRecord]) -> Option<u32> {
    sides
        .iter()
        .map(|s| s.max_players)
        .try_fold(0u32, |total, max| Some(total + max?))
}

/// What a join or an invite accept did about the player's spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOutcome {
    /// They took a spot, counted in the match's headcounts.
    Occupied,
    /// The match or their side was full, so they're on the roster with
    /// `MatchPlayerRecord::waitlisted_at` set, and nothing was counted.
    Waitlisted,
}

/// The caps a write that takes a spot is guarded on (see
/// [`take_slot_update`]), resolved by the caller from a just-read
/// [`MatchAggregate`] via [`SlotCaps::for_side`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlotCaps {
    /// The side whose `player_count` the spot counts toward: `None` for an
    /// unassigned player, and for a side the match doesn't have.
    pub side_id: Option<String>,
    /// That side's `max_players`.
    pub side_max_players: Option<u32>,
    /// The match's [`effective_max_players`].
    pub total_max_players: Option<u32>,
}

impl SlotCaps {
    /// The caps for a spot on `side_id` (`None` = unassigned). A side id the
    /// match doesn't have counts as unassigned: a roster row can carry one
    /// (`update_match`'s `side_assignments` doesn't validate it), and an `ADD`
    /// to a missing `sides.<id>` is an invalid document path that would cancel
    /// the whole transaction.
    pub fn for_side(agg: &MatchAggregate, side_id: Option<&str>) -> Self {
        let side = side_id.and_then(|sid| agg.sides.iter().find(|s| s.side_id == sid));
        SlotCaps {
            side_id: side.map(|s| s.side_id.clone()),
            side_max_players: side.and_then(|s| s.max_players),
            total_max_players: agg.effective_max_players(),
        }
    }

    /// Whether `counts` leave room for one more player under these caps. The
    /// same question [`take_slot_update`]'s guard asks of the stored counters,
    /// put to a fresh recount instead (see [`Dao::recount_has_room`]).
    fn has_room(&self, counts: &RosterCounts) -> bool {
        let total_has_room = self
            .total_max_players
            .is_none_or(|max| counts.total_player_count < u64::from(max));
        let side_has_room = match (&self.side_id, self.side_max_players) {
            (Some(side_id), Some(max)) => {
                counts.side_player_counts.get(side_id).copied().unwrap_or(0) < max
            }
            _ => true,
        };
        total_has_room && side_has_room
    }
}

/// The meta-item update that takes one spot: `ADD` 1 to `total_player_count`,
/// and to `caps.side_id`'s `player_count` when there is one. Conditioned on
/// the match existing, so one deleted since the read isn't re-created as a
/// bare counter stub, and on every cap in `caps` still having room.
///
/// The one builder behind every write that takes a spot (a join, an invite
/// accept, moving someone in off the waitlist), so none of them can take a
/// match over its cap. Accepting an invite used to build its own `ADD` with no
/// cap check, and could.
pub(super) fn take_slot_update(table: &str, match_id: &str, caps: &SlotCaps) -> DaoResult<Update> {
    let mut names = HashMap::from([("#pk".to_string(), ATTR_PK.to_string())]);
    let mut values = HashMap::from([(":one".to_string(), AttributeValue::N("1".into()))]);
    // A single `ADD` section, comma-separated — DynamoDB rejects an
    // `UpdateExpression` with more than one `ADD` keyword.
    let mut add_clauses = vec!["total_player_count :one"];
    let mut conditions = vec!["attribute_exists(#pk)"];
    if let Some(max) = caps.total_max_players {
        conditions
            .push("(attribute_not_exists(total_player_count) OR total_player_count < :totalmax)");
        values.insert(":totalmax".into(), AttributeValue::N(max.to_string()));
    }
    if let Some(side_id) = &caps.side_id {
        names.insert("#sid".into(), side_id.clone());
        add_clauses.push("sides.#sid.player_count :one");
        if let Some(max) = caps.side_max_players {
            conditions.push(
                "(attribute_not_exists(sides.#sid.max_players) OR sides.#sid.player_count < :sidemax)",
            );
            values.insert(":sidemax".into(), AttributeValue::N(max.to_string()));
        }
    }
    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
        .key(ATTR_SK, s(Sk::Meta.to_string()))
        .update_expression(format!("ADD {}", add_clauses.join(", ")))
        .condition_expression(conditions.join(" AND "))
        .set_expression_attribute_names(Some(names))
        .set_expression_attribute_values(Some(values))
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))
}

/// A match plus its sides and players, assembled from one collection query.
/// Excludes the live-scoring score record, likes and comments (fetched separately).
#[derive(Debug)]
pub struct MatchAggregate {
    pub match_: MatchRecord,
    pub sides: Vec<MatchSideRecord>,
    pub players: Vec<MatchPlayerRecord>,
}

impl MatchAggregate {
    /// See [`effective_max_players`].
    pub fn effective_max_players(&self) -> Option<u32> {
        effective_max_players(&self.sides)
    }
}

/// A match plus its sides, *without* players — for contexts (the feed) that
/// don't render the roster and shouldn't pay for querying it. See
/// [`Dao::batch_get_match_summaries`].
#[derive(Debug)]
pub struct MatchSummary {
    pub match_: MatchRecord,
    pub sides: Vec<MatchSideRecord>,
}

impl Dao {
    /// Create a match with its sides (embedded on the meta record —
    /// `player_count`/`roster_preview` are filled in here from `players`,
    /// overwriting whatever the caller set) and players, in a single
    /// transaction. `Conflict` if the match id already exists.
    ///
    /// Note: DynamoDB caps a transaction at 100 items, so a match with a very
    /// large roster would need chunking — not handled here (fine for real
    /// team sizes). Feed fan-out happens asynchronously off the stream, not here.
    #[tracing::instrument(
        skip(self, match_, players),
        fields(match_id = %match_.id, sides = match_.sides.len(), players = players.len())
    )]
    pub async fn create_match(
        &self,
        match_: &MatchRecord,
        players: &[MatchPlayerRecord],
    ) -> DaoResult<()> {
        let mut match_ = match_.clone();
        for side in match_.sides.values_mut() {
            let (player_count, roster_preview) = side_roster(&side.side_id, players);
            side.player_count = player_count;
            side.roster_preview = roster_preview;
        }
        // Seed the atomic headcount `Dao::join_match_tx` maintains from here
        // on — every player at creation who takes a spot, side-assigned or
        // not. Invitees are still pending, so they don't.
        match_.total_player_count = total_player_count(players);
        let match_ = match_;

        let meta_item = to_item(
            &Pk::Match(match_.id.clone()),
            &Sk::Meta,
            TYPE_MATCH,
            &match_,
        )?;

        let put_meta = Put::builder()
            .table_name(self.table())
            .set_item(Some(meta_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_meta).build());

        for player in players {
            let put = Put::builder()
                .table_name(self.table())
                .set_item(Some(self.match_player_item(&match_.id, player)?))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            tx = tx.transact_items(TransactWriteItem::builder().put(put).build());

            // Write the participant's *own* feed row synchronously so a match
            // they're playing in shows on their feed the moment it's created.
            // Only for already-joined players — a linked user with no pending
            // invitation (the creator who opted to play / a self-added player).
            // Invitees get their row when they accept; followers via async
            // fan-out. Idempotent on the feed sort key, so the fan-out re-write
            // is harmless.
            let joined = player.invitation.is_none();
            if let (Some(uid), true) = (&player.user_id, joined) {
                let feed_item = self.feed_item(
                    uid,
                    &match_.id,
                    &match_.starts_at,
                    &match_.created_at,
                    &AudienceMember {
                        viewer_side_id: player.side_id.clone(),
                        ..Default::default()
                    },
                )?;
                let feed_put = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(feed_item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                tx = tx.transact_items(TransactWriteItem::builder().put(feed_put).build());
            }
        }

        match tx.send().await {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("match {} already exists", match_.id),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch the match aggregate (meta + sides + players). `None` if the meta
    /// item is absent. Likes/comments/submissions/live-scoring score record
    /// are deliberately not loaded here — fetch via their own paginated ops.
    ///
    /// Sides are embedded on the meta record, so this is just a `GetItem` for
    /// meta (which yields sides for free) plus a `begins_with(SK, "PLAYER#")`
    /// query for players. That query is still scoped rather than a
    /// whole-partition read — it reads only the handful of player items, not
    /// the potentially large COMMENT#/LIKE#/SCORESUB# ranges, and avoids the
    /// 1 MB single-Query-page trap those could cause. `BatchGetItem` isn't an
    /// option there — player ids aren't known ahead of the read.
    #[tracing::instrument(skip(self))]
    pub async fn get_match(&self, match_id: &str) -> DaoResult<Option<MatchAggregate>> {
        let pk = Pk::Match(match_id.into());

        // Meta first: if the match doesn't exist, skip the players read.
        let meta_out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(pk.to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        let Some(meta_item) = meta_out.item else {
            return Ok(None);
        };
        let match_: MatchRecord = from_item(meta_item)?;

        let players: Vec<MatchPlayerRecord> = self
            .query_match_collection(match_id, &Sk::player_prefix())
            .await?;
        let sides = sorted_sides(&match_.sides);

        Ok(Some(MatchAggregate {
            match_,
            sides,
            players,
        }))
    }

    /// Fetch many matches' meta items by id in one round-trip, keyed by id.
    /// Missing ids are simply absent from the map (mirrors [`get_match`]
    /// returning `None`), and duplicate ids collapse.
    ///
    /// Each meta item is an exact-key point read, so this collapses what would
    /// be N `GetItem`s into one `BatchGetItem` (see [`batch_get_all`] for the
    /// unprocessed-key retry). Callers must pass at most `BATCH_GET_MAX` ids —
    /// every current caller hydrates a single, service-capped page.
    ///
    /// [`batch_get_all`]: Dao::batch_get_all
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_match_metas(
        &self,
        match_ids: &[String],
    ) -> DaoResult<HashMap<String, MatchRecord>> {
        let mut seen = std::collections::HashSet::new();
        let keys: Vec<_> = match_ids
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .map(|id| {
                HashMap::from([
                    (ATTR_PK.to_string(), s(Pk::Match(id.clone()).to_string())),
                    (ATTR_SK.to_string(), s(Sk::Meta.to_string())),
                ])
            })
            .collect();

        let items = self.batch_get_all(keys, None).await?;
        let mut out = HashMap::with_capacity(items.len());
        for item in items {
            let record: MatchRecord = from_item(item)?;
            out.insert(record.id.clone(), record);
        }
        Ok(out)
    }

    /// Fetch specific players from one match by id, in one round-trip. Each
    /// player's key (`PK = MATCH#<matchId>`, `SK = PLAYER#<playerId>`) is
    /// already known to the caller (e.g. a live-scoring `next_ball_context`'s
    /// striker/non-striker ids), so this is a `BatchGetItem` of exact keys —
    /// not the `begins_with(SK, "PLAYER#")` `Query` `get_match` uses for the
    /// *whole* roster. Resolving a couple of names this way stays flat-cost
    /// regardless of squad size, unlike a full roster query. Missing ids are
    /// simply absent from the map.
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_match_players(
        &self,
        match_id: &str,
        player_ids: &[String],
    ) -> DaoResult<HashMap<String, MatchPlayerRecord>> {
        let mut seen = std::collections::HashSet::new();
        let keys: Vec<_> = player_ids
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .map(|id| {
                HashMap::from([
                    (
                        ATTR_PK.to_string(),
                        s(Pk::Match(match_id.into()).to_string()),
                    ),
                    (ATTR_SK.to_string(), s(Sk::Player(id.clone()).to_string())),
                ])
            })
            .collect();

        let items = self.batch_get_all(keys, None).await?;
        let mut out = HashMap::with_capacity(items.len());
        for item in items {
            let record: MatchPlayerRecord = from_item(item)?;
            out.insert(record.player_id.clone(), record);
        }
        Ok(out)
    }

    /// Fetch specific players spanning possibly many *different* matches, in
    /// one round-trip — the multi-match counterpart to
    /// [`batch_get_match_players`]. `BatchGetItem` doesn't care that the keys
    /// span different partitions (a different match's `PK` each), so this is
    /// still one request-shape whether `keys` names one match or a whole
    /// feed/search page's worth. Used to resolve a page of matches'
    /// `confirmed_score`/`pending_score` player ids without the cost scaling
    /// with page size — see `Api::hydrate_score_players_across_matches`.
    /// Keyed by `(match_id, player_id)` in the result (unlike the
    /// single-match version, a bare player id isn't unique across matches).
    /// Missing pairs are simply absent from the map.
    ///
    /// [`batch_get_match_players`]: Dao::batch_get_match_players
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_players_across_matches(
        &self,
        keys: &[(String, String)],
    ) -> DaoResult<HashMap<(String, String), MatchPlayerRecord>> {
        let mut seen = std::collections::HashSet::new();
        let dynamo_keys: Vec<_> = keys
            .iter()
            .filter(|key| seen.insert((*key).clone()))
            .map(|(match_id, player_id)| {
                HashMap::from([
                    (
                        ATTR_PK.to_string(),
                        s(Pk::Match(match_id.clone()).to_string()),
                    ),
                    (
                        ATTR_SK.to_string(),
                        s(Sk::Player(player_id.clone()).to_string()),
                    ),
                ])
            })
            .collect();

        let items = self.batch_get_all(dynamo_keys, None).await?;
        let mut out = HashMap::with_capacity(items.len());
        for item in items {
            let Pk::Match(match_id) = item_pk(&item)? else {
                return Err(DaoError::Malformed(
                    "batch_get_players_across_matches returned a non-match item".into(),
                ));
            };
            let record: MatchPlayerRecord = from_item(item)?;
            out.insert((match_id, record.player_id.clone()), record);
        }
        Ok(out)
    }

    /// Fetch match summaries (meta + sides, *no* players) for many matches at
    /// once — the feed's and search's hydration path, neither of which
    /// renders a full roster (see [`MatchSummary`]). Missing ids are simply
    /// absent from the map. Callers must pass at most `BATCH_GET_MAX` ids.
    ///
    /// Sides are embedded on the meta record, so unlike `get_match`'s players
    /// query, there's nothing left here that needs a per-match `Query` at
    /// all: the meta reads already collapse into a single `BatchGetItem` via
    /// [`batch_get_match_metas`], and sides ride along on that same read.
    /// This function's cost is flat regardless of page size.
    ///
    /// [`batch_get_match_metas`]: Dao::batch_get_match_metas
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_match_summaries(
        &self,
        match_ids: &[String],
    ) -> DaoResult<HashMap<String, MatchSummary>> {
        let metas = self.batch_get_match_metas(match_ids).await?;
        Ok(metas
            .into_iter()
            .map(|(id, match_)| {
                let sides = sorted_sides(&match_.sides);
                (id, MatchSummary { match_, sides })
            })
            .collect())
    }

    /// Read every item in a match's partition whose SK starts with `sk_prefix`
    /// (e.g. `SIDE#`, `PLAYER#`), draining all query pages so a large collection
    /// is never truncated at the 1 MB page limit. Deserializes each into `T`.
    #[tracing::instrument(skip(self))]
    pub(super) async fn query_match_collection<T: serde::de::DeserializeOwned>(
        &self,
        match_id: &str,
        sk_prefix: &str,
    ) -> DaoResult<Vec<T>> {
        self.read_match_collection(match_id, sk_prefix, false).await
    }

    /// [`Self::query_match_collection`], with the read consistency chosen by
    /// the caller. Only the roster recount (`refresh_side_roster_previews`)
    /// asks for strong consistency: it writes what it reads straight back, and
    /// its guard can't catch a player row that a lagging read simply missed.
    async fn read_match_collection<T: serde::de::DeserializeOwned>(
        &self,
        match_id: &str,
        sk_prefix: &str,
        consistent_read: bool,
    ) -> DaoResult<Vec<T>> {
        let pk = Pk::Match(match_id.into()).to_string();
        let mut items = Vec::new();
        let mut start_key = None;
        loop {
            let out = self
                .client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(pk.clone()))
                .expression_attribute_values(":sk", s(sk_prefix))
                .consistent_read(consistent_read)
                .set_exclusive_start_key(start_key)
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;

            for item in out.items.unwrap_or_default() {
                items.push(from_item(item)?);
            }

            match out.last_evaluated_key {
                Some(k) => start_key = Some(k),
                None => break,
            }
        }
        Ok(items)
    }

    /// Update a match's mutable meta fields. Any `Some` field is written; `name`,
    /// `description`, `status`, `starts_at`, `location` (Some(None) clears it),
    /// and the resolved `confirmed_score`/`pending_score` blobs. `NotFound` if
    /// the match is absent.
    ///
    /// `side_names` renames one or more sides in the same `UpdateItem` call as
    /// everything else here — both target the same item, so folding it in
    /// (rather than a second call) makes a request that edits, say, the
    /// match's `name` and a side's `name` together atomic: either both land
    /// or neither does, instead of one succeeding and the other failing
    /// independently. `(side_id, Some(name))` sets that side's custom name;
    /// `(side_id, None)` removes it, falling back at read time to the
    /// priority chain `Api::resolve_side_names` implements (sole player, then
    /// team, then a neutral default). An empty slice touches no sides.
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(skip(self))]
    pub async fn update_match_meta(
        &self,
        match_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        starts_at: Option<&str>,
        confirmed_score: Option<ConfirmedScoreRecord>,
        pending_score: Option<Option<PendingScoreRecord>>,
        // Replace the header photos, in order. `None` leaves them unchanged;
        // `Some([])` clears them (removes the attribute); `Some([..])`
        // overwrites.
        header_photos: Option<Vec<HeaderPhotoRecord>>,
        // Replace the match format. `None` leaves it unchanged; `Some(value)`
        // overwrites. No "clear" case yet (Phase 1 doesn't need one — a
        // match's sport, and so its format shape, doesn't change).
        format: Option<MatchFormatRecord>,
        side_names: &[(String, Option<String>)],
    ) -> DaoResult<()> {
        let mut set: Vec<String> = Vec::new();
        let mut remove: Vec<String> = Vec::new();
        let mut names: std::collections::HashMap<String, String> = Default::default();
        let mut values: std::collections::HashMap<String, AttributeValue> = Default::default();

        let set_str =
            |field: &str,
             alias: &str,
             val: &str,
             set: &mut Vec<String>,
             names: &mut std::collections::HashMap<String, String>,
             values: &mut std::collections::HashMap<String, AttributeValue>| {
                set.push(format!("#{alias} = :{alias}"));
                names.insert(format!("#{alias}"), field.to_string());
                values.insert(format!(":{alias}"), s(val));
            };

        if let Some(v) = name {
            set_str("name", "name", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = description {
            set_str("description", "desc", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = status {
            set_str("status", "status", v, &mut set, &mut names, &mut values);
        }
        if let Some(v) = starts_at {
            set_str("starts_at", "starts", v, &mut set, &mut names, &mut values);
        }
        if let Some(cs) = confirmed_score {
            set.push("#cs = :cs".into());
            names.insert("#cs".into(), "confirmed_score".into());
            values.insert(":cs".into(), to_attr(&cs)?);
        }
        match pending_score {
            Some(Some(ps)) => {
                set.push("#ps = :ps".into());
                names.insert("#ps".into(), "pending_score".into());
                values.insert(":ps".into(), to_attr(&ps)?);
            }
            Some(None) => {
                remove.push("#ps".into());
                names.insert("#ps".into(), "pending_score".into());
            }
            None => {}
        }
        match header_photos {
            Some(photos) if !photos.is_empty() => {
                set.push("#hp = :hp".into());
                names.insert("#hp".into(), "header_photos".into());
                values.insert(":hp".into(), to_attr(&photos)?);
            }
            // Clearing: remove the attribute so a read defaults to an empty Vec.
            Some(_) => {
                remove.push("#hp".into());
                names.insert("#hp".into(), "header_photos".into());
            }
            None => {}
        }
        if let Some(fmt) = format {
            set.push("#fmt = :fmt".into());
            names.insert("#fmt".into(), "format".into());
            values.insert(":fmt".into(), to_attr(&fmt)?);
        }
        if !side_names.is_empty() {
            // Same literal attribute name ("name") as the top-level `#name`
            // alias above — reinserting it with the same value is harmless,
            // and lets a top-level rename and a side rename share one alias.
            names.insert("#name".into(), "name".into());
            for (i, (side_id, side_name)) in side_names.iter().enumerate() {
                let side_alias = format!("#s{i}");
                names.insert(side_alias.clone(), side_id.clone());
                match side_name {
                    Some(n) => {
                        let value_alias = format!(":n{i}");
                        set.push(format!("sides.{side_alias}.#name = {value_alias}"));
                        values.insert(value_alias, s(n));
                    }
                    None => {
                        remove.push(format!("sides.{side_alias}.#name"));
                    }
                }
            }
        }

        if set.is_empty() && remove.is_empty() {
            return Ok(());
        }

        let mut expr = String::new();
        if !set.is_empty() {
            expr.push_str("SET ");
            expr.push_str(&set.join(", "));
        }
        if !remove.is_empty() {
            if !expr.is_empty() {
                expr.push(' ');
            }
            expr.push_str("REMOVE ");
            expr.push_str(&remove.join(", "));
        }
        names.insert("#pk".into(), ATTR_PK.into());

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .update_expression(expr)
            .condition_expression("attribute_exists(#pk)")
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(if values.is_empty() {
                None
            } else {
                Some(values)
            })
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("match {match_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Update a match's join settings: `allow_unassigned`, and/or one or more
    /// sides' `max_players`/`team_join_enabled`. Kept separate from
    /// `update_match_meta` (rather than folded into its one big `UpdateItem`)
    /// since these settings aren't coupled to the rest of that call's fields
    /// the way `side_names` is — no atomicity is lost by it being its own
    /// call. `NotFound` if the match is absent.
    #[tracing::instrument(skip(self))]
    pub async fn update_match_join_settings(
        &self,
        match_id: &str,
        allow_unassigned: Option<bool>,
        side_join_settings: &[(String, Option<u32>, bool)],
    ) -> DaoResult<()> {
        if allow_unassigned.is_none() && side_join_settings.is_empty() {
            return Ok(());
        }

        let mut set: Vec<String> = Vec::new();
        let mut remove: Vec<String> = Vec::new();
        let mut names: std::collections::HashMap<String, String> = Default::default();
        let mut values: std::collections::HashMap<String, AttributeValue> = Default::default();
        names.insert("#pk".into(), ATTR_PK.into());

        if let Some(allow) = allow_unassigned {
            set.push("allow_unassigned = :au".into());
            values.insert(":au".into(), AttributeValue::Bool(allow));
        }
        for (i, (side_id, max_players, team_join_enabled)) in side_join_settings.iter().enumerate()
        {
            let side_alias = format!("#s{i}");
            names.insert(side_alias.clone(), side_id.clone());
            match max_players {
                Some(max) => {
                    let value_alias = format!(":m{i}");
                    set.push(format!("sides.{side_alias}.max_players = {value_alias}"));
                    values.insert(value_alias, AttributeValue::N(max.to_string()));
                }
                None => {
                    remove.push(format!("sides.{side_alias}.max_players"));
                }
            }
            let tje_alias = format!(":tje{i}");
            set.push(format!(
                "sides.{side_alias}.team_join_enabled = {tje_alias}"
            ));
            values.insert(tje_alias, AttributeValue::Bool(*team_join_enabled));
        }

        let mut expr = String::new();
        if !set.is_empty() {
            expr.push_str("SET ");
            expr.push_str(&set.join(", "));
        }
        if !remove.is_empty() {
            if !expr.is_empty() {
                expr.push(' ');
            }
            expr.push_str("REMOVE ");
            expr.push_str(&remove.join(", "));
        }

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .update_expression(expr)
            .condition_expression("attribute_exists(#pk)")
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(if values.is_empty() {
                None
            } else {
                Some(values)
            })
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("match {match_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Move the `Owner` role from one player to another, in one transaction
    /// — mirrors `Dao::transfer_team_ownership` exactly (same reasoning: the
    /// promote and demote either both land or neither does, so the match is
    /// never briefly ownerless or briefly double-owned). `NotFound` if either
    /// player doesn't exist. The caller is responsible for every
    /// business-rule check (caller is the current owner, `to` is an accepted
    /// player, `to` isn't already the owner) — this just moves the role.
    #[tracing::instrument(skip(self))]
    pub async fn transfer_match_ownership(
        &self,
        match_id: &str,
        from_player_id: &str,
        to_player_id: &str,
    ) -> DaoResult<()> {
        let promote = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Player(to_player_id.into()).to_string()))
            .update_expression("SET #role = :owner")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#role", "role")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":owner", s("owner"))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let demote = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Player(from_player_id.into()).to_string()))
            .update_expression("SET #role = :admin")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#role", "role")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":admin", s("admin"))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(promote).build())
            .transact_items(TransactWriteItem::builder().update(demote).build())
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::NotFound(
                format!("player {from_player_id} or {to_player_id} on match {match_id}"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Join a match: put `player` (fully built by the caller — `side_id`,
    /// `role`, `joined_via` set, `waitlisted_at` not) into a spot if there's
    /// one, or onto the waitlist if the match or their side is full. Either way
    /// this also writes the joiner's own feed row (same as
    /// `create_match`/`accept_invitation_tx`), so the match shows up on their
    /// feed straight away: a waitlisted joiner is on the roster too, waiting on
    /// a game they want to follow.
    ///
    /// **Capacity is decided by the write, not by a read.** The player row goes
    /// in with a cap-guarded `ADD` to the headcounts ([`take_slot_update`],
    /// guarded on `caps`, which the caller resolves from a just-read
    /// [`MatchAggregate`]). If the guard fails, the match is recounted
    /// ([`Self::recount_has_room`]) and a spot that turns up is tried for once
    /// more. Otherwise the player is written with `waitlisted_at` set and
    /// nothing counted.
    ///
    /// There's deliberately no "is it full?" check before any of this. There
    /// used to be one, which turned joiners away with a 409, and this guard was
    /// only its last-moment race backstop. Now that full means the waitlist
    /// rather than an error, a join that loses the race for the last spot and a
    /// join into a match that was already full should land in the same place,
    /// so they take the same path.
    #[tracing::instrument(skip(self, player, caps), fields(player_id = %player.player_id))]
    pub async fn join_match_tx(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        caps: &SlotCaps,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<SlotOutcome> {
        if self
            .try_join_into_slot(match_id, player, caps, starts_at, now)
            .await?
            || (self.recount_has_room(match_id, caps).await?
                && self
                    .try_join_into_slot(match_id, player, caps, starts_at, now)
                    .await?)
        {
            return Ok(SlotOutcome::Occupied);
        }

        let waitlisted = MatchPlayerRecord {
            waitlisted_at: Some(now.to_string()),
            ..player.clone()
        };
        let mut tx = self.client.transact_write_items().transact_items(
            TransactWriteItem::builder()
                .put(self.new_match_player_put(match_id, &waitlisted)?)
                .build(),
        );
        if let Some(feed_put) = self.joiner_feed_put(match_id, player, starts_at, now)? {
            tx = tx.transact_items(TransactWriteItem::builder().put(feed_put).build());
        }
        match tx.send().await {
            Ok(_) => Ok(SlotOutcome::Waitlisted),
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(duplicate_player(match_id, player))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// One attempt at a spot for [`Self::join_match_tx`]: the player row, the
    /// cap-guarded `ADD` and the joiner's feed row, in one transaction.
    /// `Ok(false)` if the cap guard failed, and nothing was written.
    async fn try_join_into_slot(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        caps: &SlotCaps,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<bool> {
        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(
                TransactWriteItem::builder()
                    .put(self.new_match_player_put(match_id, player)?)
                    .build(),
            )
            .transact_items(
                TransactWriteItem::builder()
                    .update(take_slot_update(self.table(), match_id, caps)?)
                    .build(),
            );
        if let Some(feed_put) = self.joiner_feed_put(match_id, player, starts_at, now)? {
            tx = tx.transact_items(TransactWriteItem::builder().put(feed_put).build());
        }
        match tx.send().await {
            Ok(_) => Ok(true),
            Err(e) if super::item_condition_failed(&e, 1) => Ok(false),
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(duplicate_player(match_id, player))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// A new roster row, guarded on not overwriting one. The player id is
    /// freshly minted, so only a collision could trip it.
    fn new_match_player_put(&self, match_id: &str, player: &MatchPlayerRecord) -> DaoResult<Put> {
        Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))
    }

    /// A joiner's own feed row, if they're a linked user (see
    /// [`Self::join_match_tx`]).
    fn joiner_feed_put(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<Option<Put>> {
        let Some(uid) = &player.user_id else {
            return Ok(None);
        };
        let feed_item = self.feed_item(
            uid,
            match_id,
            starts_at,
            now,
            &AudienceMember {
                viewer_side_id: player.side_id.clone(),
                ..Default::default()
            },
        )?;
        Put::builder()
            .table_name(self.table())
            .set_item(Some(feed_item))
            .build()
            .map(Some)
            .map_err(|e| DaoError::Dynamo(e.to_string()))
    }

    /// Move a waitlisted player into a spot: an organiser's manual move off the
    /// waitlist. In one transaction, clear `player`'s `waitlisted_at` and take
    /// the spot with the same cap-guarded `ADD` as a join ([`take_slot_update`],
    /// on `caps`). A free spot is required, so this never takes a match past
    /// its cap: an organiser who wants more players raises a cap first. As with
    /// a join, a failed cap guard is recounted before it's believed
    /// ([`Self::recount_has_room`]).
    ///
    /// The row write is an `UpdateItem` (`REMOVE waitlisted_at`), so it can't
    /// undo a concurrent change to the row's other fields. It's guarded on the
    /// player still waiting, so a double tap (or two admins at once) counts the
    /// spot once, and on their `side_id` still being the one read, since that's
    /// the side `caps` counts the spot to. The reverse isn't guarded: a
    /// whole-row rewrite built from a read taken before the move (the accept
    /// saga's re-link, `update_match`'s side reassignment) can put
    /// `waitlisted_at` back. Each of those recounts straight afterwards, so the
    /// counts still match the rows and the player just shows as waiting again.
    /// Guarding every whole-row writer against a race that needs an organiser
    /// to act within the same second wasn't worth it.
    ///
    /// `NotFound` if `player` is no longer waiting as read (already moved in,
    /// gone, or put on another side); `Conflict` if there's no free spot.
    #[tracing::instrument(skip(self, player, caps), fields(player_id = %player.player_id))]
    pub async fn move_in_waitlisted_player(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        caps: &SlotCaps,
    ) -> DaoResult<()> {
        if self.try_move_into_slot(match_id, player, caps).await?
            || (self.recount_has_room(match_id, caps).await?
                && self.try_move_into_slot(match_id, player, caps).await?)
        {
            return Ok(());
        }
        Err(DaoError::Conflict(format!(
            "match {match_id} has no free spot"
        )))
    }

    /// One attempt at [`Self::move_in_waitlisted_player`]. `Ok(false)` if the
    /// cap guard failed, and nothing was written.
    async fn try_move_into_slot(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        caps: &SlotCaps,
    ) -> DaoResult<bool> {
        let leave_waitlist = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Player(player.player_id.clone()).to_string()))
            .update_expression("REMOVE waitlisted_at")
            .expression_attribute_names("#pk", ATTR_PK);
        let leave_waitlist = match &player.side_id {
            Some(side_id) => leave_waitlist
                .condition_expression(
                    "attribute_exists(#pk) AND attribute_exists(waitlisted_at) AND side_id = :sid",
                )
                .expression_attribute_values(":sid", s(side_id)),
            None => leave_waitlist.condition_expression(
                "attribute_exists(#pk) AND attribute_exists(waitlisted_at) \
                 AND attribute_not_exists(side_id)",
            ),
        }
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().update(leave_waitlist).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(take_slot_update(self.table(), match_id, caps)?)
                    .build(),
            )
            .send()
            .await;
        match result {
            Ok(_) => Ok(true),
            // Checked first: a player who's no longer waiting has nothing to
            // move in, whether or not there's room.
            Err(e) if super::item_condition_failed(&e, 0) => Err(DaoError::NotFound(format!(
                "waitlisted player {} in match {match_id}",
                player.player_id
            ))),
            Err(e) if super::item_condition_failed(&e, 1) => Ok(false),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Recount the match's headcounts (healing any drift — see
    /// [`Self::refresh_side_roster_previews`]) and say whether the fresh
    /// counts leave room under `caps`. `false` if the match is gone.
    ///
    /// What every write that takes a spot does when its cap guard fails,
    /// before believing it. Stored counts that drifted high (written before
    /// pending invites stopped counting, or before leaving gave a spot back)
    /// fail the guard exactly as a full match does, and a match that only
    /// *looks* full gets no other roster writes that would heal it: without
    /// this it would waitlist every joiner and refuse every move-in for good.
    pub(super) async fn recount_has_room(
        &self,
        match_id: &str,
        caps: &SlotCaps,
    ) -> DaoResult<bool> {
        Ok(self
            .refresh_side_roster_previews(match_id)
            .await?
            .is_some_and(|counts| caps.has_room(&counts)))
    }

    /// Add or update a single match player (roster reconciliation / late adds).
    #[tracing::instrument(skip(self, player), fields(player_id = %player.player_id))]
    pub async fn put_match_player(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
    ) -> DaoResult<()> {
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Remove players from a match's roster entirely (not just unassign their
    /// side — see `put_match_player` with `side_id: None` for that), in one
    /// batch delete (`BatchWriteItem`, `BATCH_WRITE_MAX` items/request — see
    /// `dao::batch`) rather than a `DeleteItem` per player. Idempotent
    /// (deleting a missing player is a no-op); a no-op itself for an empty
    /// `player_ids`.
    #[tracing::instrument(skip(self))]
    pub async fn remove_match_players(
        &self,
        match_id: &str,
        player_ids: &[String],
    ) -> DaoResult<()> {
        for chunk in player_ids.chunks(super::batch::BATCH_WRITE_MAX) {
            let mut requests = Vec::with_capacity(chunk.len());
            for player_id in chunk {
                let key = HashMap::from([
                    (
                        ATTR_PK.to_string(),
                        s(Pk::Match(match_id.into()).to_string()),
                    ),
                    (
                        ATTR_SK.to_string(),
                        s(Sk::Player(player_id.clone()).to_string()),
                    ),
                ]);
                let delete = DeleteRequest::builder()
                    .set_key(Some(key))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                requests.push(WriteRequest::builder().delete_request(delete).build());
            }
            self.flush_batch_write(requests).await?;
        }
        Ok(())
    }

    /// Recompute and store the match's headcounts — `total_player_count`, and
    /// every side's `player_count`/`roster_preview` — from its *current*
    /// player collection, counting only players who take a spot
    /// ([`MatchPlayerRecord::occupies_slot`]). Call after any write that can
    /// change who's on the roster, which side they're on, or who they are —
    /// removals, `put_match_player` (late adds, side moves), invitation
    /// acceptance (linking external → user). Removals rely on it: nothing
    /// else ever brings a count back down. `create_match` doesn't need this:
    /// it computes the same thing inline, from the roster it already has in
    /// memory, within the same transaction. Returns the counts it stored, or
    /// `None` if the match is gone.
    ///
    /// Because it recounts rather than adjusts, this is also what heals a
    /// count that has drifted from the roster (e.g. one written before pending
    /// invites stopped counting) — every write that takes a spot runs it when
    /// its cap guard fails, before waitlisting or refusing anyone
    /// ([`Self::recount_has_room`]), since a match that only *looks* full gets
    /// no other roster writes that would heal it.
    ///
    /// **Concurrency.** The same counters are `ADD`ed, atomically and
    /// cap-guarded, by `Dao::join_match_tx`, `Dao::accept_invitation_tx` and
    /// `Dao::move_in_waitlisted_player`. A
    /// `SET` computed from a read taken before one of those commits would
    /// silently overwrite it — un-counting a player who just got in, and
    /// letting the cap be exceeded. So the `SET` is conditioned on every
    /// counter it overwrites still holding the value read (every `ADD` bumps
    /// `total_player_count`, so that guard alone catches a concurrent join or
    /// accept; the per-side guards also catch churn that happens to leave the
    /// total where it was), and a failed guard means re-read and recount, up
    /// to [`ROSTER_RECOUNT_ATTEMPTS`] times.
    ///
    /// The order of the two reads is load-bearing, and both are strongly
    /// consistent. Counters first, then players: an `ADD` commits in the same
    /// transaction as its player row, so any row the players query sees was
    /// either already counted in the values read or will trip the guard. The
    /// other way round, a join landing between the two reads would be in the
    /// guarded values but missing from the recount — and the write would go
    /// through, dropping it.
    ///
    /// One consistent `GetItem` (meta, projected to the counters) + one
    /// consistent `Query` (players — ids aren't known ahead of the read) + one
    /// conditional `UpdateItem` that sets every counter in a single call,
    /// however many sides there are. Runs on the rarer roster-mutating write
    /// paths, never on a feed read — the whole point is for the feed to
    /// *avoid* re-deriving this live.
    #[tracing::instrument(skip(self))]
    pub async fn refresh_side_roster_previews(
        &self,
        match_id: &str,
    ) -> DaoResult<Option<RosterCounts>> {
        for attempt in 0..ROSTER_RECOUNT_ATTEMPTS {
            super::batch::backoff(attempt).await;
            let Some(stored) = self.stored_roster_counts(match_id).await? else {
                return Ok(None); // Match gone.
            };
            let players = self
                .read_match_collection::<MatchPlayerRecord>(match_id, &Sk::player_prefix(), true)
                .await?;
            let write = recount_write(&stored, &players)?;

            let result = self
                .client
                .update_item()
                .table_name(self.table())
                .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
                .key(ATTR_SK, s(Sk::Meta.to_string()))
                .update_expression(write.update_expression)
                .condition_expression(write.condition_expression)
                .set_expression_attribute_names(Some(write.names))
                .set_expression_attribute_values(Some(write.values))
                .send()
                .await;
            match result {
                Ok(_) => return Ok(Some(write.counts)),
                // A counter moved under us (or the match was deleted) —
                // recount from a fresh read.
                Err(e) if is_update_conditional_failure(&e) => continue,
                Err(e) => return Err(DaoError::Dynamo(e.to_string())),
            }
        }
        Err(DaoError::Conflict(format!(
            "match {match_id}'s roster kept changing; gave up recounting after \
             {ROSTER_RECOUNT_ATTEMPTS} attempts"
        )))
    }

    /// The counters a recount guards its write on (see
    /// [`refresh_side_roster_previews`]), without pulling the rest of the meta
    /// item. Strongly consistent — that method explains why. `None` if the
    /// match doesn't exist.
    async fn stored_roster_counts(&self, match_id: &str) -> DaoResult<Option<StoredRosterCounts>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .projection_expression("sides, total_player_count")
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        out.item.map(from_item).transpose()
    }

    /// Fetch a match's live-scoring score record. `None` if none recorded.
    #[tracing::instrument(skip(self))]
    pub async fn get_match_score(
        &self,
        match_id: &str,
        sport: &str,
    ) -> DaoResult<Option<MatchScoreRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Score(sport.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Write (overwrite) a match's live-scoring score record.
    #[tracing::instrument(skip(self, record), fields(sport = %record.sport))]
    pub async fn put_match_score(
        &self,
        match_id: &str,
        record: &MatchScoreRecord,
    ) -> DaoResult<()> {
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::Score(record.sport.clone()),
            TYPE_MATCH_SCORE,
            record,
        )?;
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Build a match-player item, projecting players with a linked user into GSI1
    /// (`UMATCHES#<userId>`) — not used for the feed (that's fan-out), but handy
    /// for "matches involving me" style reverse lookups if needed later. Players
    /// with no user id (external) are not projected.
    pub(super) fn match_player_item(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
    ) -> DaoResult<std::collections::HashMap<String, AttributeValue>> {
        let base = to_item(
            &Pk::Match(match_id.into()),
            &Sk::Player(player.player_id.clone()),
            TYPE_MATCH_PLAYER,
            player,
        )?;
        let item = match &player.user_id {
            Some(uid) => ItemBuilder::new(base)
                .gsi1(
                    format!("UMATCHES#{uid}"),
                    Pk::Match(match_id.into()).to_string(),
                )
                .build(),
            None => base,
        };
        Ok(item)
    }
}

/// Serialize any record value into a DynamoDB AttributeValue (nested map/list).
fn to_attr<T: serde::Serialize>(value: &T) -> DaoResult<AttributeValue> {
    Ok(serde_dynamo::to_attribute_value(value)?)
}

/// A join's roster row collided with an existing one. Its player id is freshly
/// minted, so this shouldn't happen; "already on the roster" is caught by the
/// caller's own read, before a row is ever built.
fn duplicate_player(match_id: &str, player: &MatchPlayerRecord) -> DaoError {
    DaoError::Conflict(format!(
        "match {match_id} already has a player {}",
        player.player_id
    ))
}

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(max_players: Option<u32>) -> MatchSideRecord {
        MatchSideRecord {
            side_id: "s".into(),
            team_id: None,
            name: None,
            max_players,
            player_count: 0,
            roster_preview: Vec::new(),
            team_join_enabled: false,
        }
    }

    #[test]
    fn effective_max_players_sums_when_every_side_has_one() {
        assert_eq!(
            effective_max_players(&[side(Some(5)), side(Some(7))]),
            Some(12)
        );
    }

    #[test]
    fn effective_max_players_uncapped_if_any_side_is_uncapped() {
        assert_eq!(effective_max_players(&[side(Some(5)), side(None)]), None);
    }

    #[test]
    fn effective_max_players_zero_for_no_sides() {
        assert_eq!(effective_max_players(&[]), Some(0));
    }

    /// A roster row on `side_id` (`None` = unassigned) with the given embedded
    /// invitation status (`None` = no invitation: added or self-joined).
    fn player(id: &str, side_id: Option<&str>, invitation: Option<&str>) -> MatchPlayerRecord {
        use crate::dao::records::{EmbeddedInvitationRecord, InvitationKindRecord};
        MatchPlayerRecord {
            player_id: id.into(),
            user_id: Some(format!("u_{id}")),
            display_name: None,
            side_id: side_id.map(Into::into),
            is_member_of_team: None,
            invitation: invitation.map(|status| EmbeddedInvitationRecord {
                id: format!("inv_{id}"),
                status: status.into(),
                invited_by_user_id: "host".into(),
                invited_at: "2026-09-14T00:00:00Z".into(),
                responded_at: None,
                kind: InvitationKindRecord::User {
                    invited_user_id: format!("u_{id}"),
                },
            }),
            role: Default::default(),
            joined_via: None,
            waitlisted_at: None,
        }
    }

    fn stored(total: Option<u64>, sides: &[(&str, Option<u32>)]) -> StoredRosterCounts {
        StoredRosterCounts {
            total_player_count: total,
            sides: sides
                .iter()
                .map(|(id, player_count)| {
                    (
                        id.to_string(),
                        StoredSideCount {
                            player_count: *player_count,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The recount counts only players who take a spot: pending and declined
    /// invitees are left out of both their side and the match total, and an
    /// unassigned player counts toward the total only — while the side's
    /// roster preview still lists everyone placed on it. The bug this guards:
    /// the counts included every roster row, so a declined invite held a spot
    /// forever and a match could read "full" with room to spare.
    #[test]
    fn recount_counts_only_players_who_take_a_spot() {
        let players = [
            player("creator", Some("a"), None),
            player("accepted", Some("a"), Some("accepted")),
            player("pending", Some("b"), Some("pending")),
            player("declined", Some("b"), Some("declined")),
            player("joined_unassigned", None, None),
        ];
        let write = recount_write(
            &stored(Some(5), &[("a", Some(2)), ("b", Some(2))]),
            &players,
        )
        .unwrap();
        assert_eq!(
            write.counts,
            RosterCounts {
                total_player_count: 3,
                side_player_counts: HashMap::from([("a".to_string(), 2), ("b".to_string(), 0)]),
            }
        );
        let (_, preview_b) = side_roster("b", &players);
        assert_eq!(
            preview_b.len(),
            2,
            "side b's preview still shows its invitees"
        );
    }

    /// The recount's `SET` is guarded on every counter it overwrites still
    /// holding the value read — the only thing stopping a recount from undoing
    /// a `join_match_tx`/`accept_invitation_tx` `ADD` that landed after its
    /// read (un-counting a player who just got in, so the cap can be
    /// exceeded). A counter missing from an older item is guarded as absent:
    /// `= 0` never matches a missing attribute, so the recount would fail its
    /// own guard on every attempt.
    #[test]
    fn recount_write_is_guarded_on_every_counter_it_overwrites() {
        let write = recount_write(&stored(Some(7), &[("a", Some(4)), ("b", None)]), &[]).unwrap();
        assert_eq!(
            write.condition_expression,
            "attribute_exists(#pk) AND total_player_count = :read_tpc \
             AND sides.#s0.player_count = :read_pc0 \
             AND attribute_not_exists(sides.#s1.player_count)"
        );
        assert_eq!(write.values[":read_tpc"], AttributeValue::N("7".into()));
        assert_eq!(write.values[":read_pc0"], AttributeValue::N("4".into()));
        assert_eq!(write.names["#s0"], "a");
        assert_eq!(write.names["#s1"], "b");

        let legacy = recount_write(&stored(None, &[]), &[]).unwrap();
        assert_eq!(
            legacy.condition_expression,
            "attribute_exists(#pk) AND attribute_not_exists(total_player_count)"
        );
    }

    /// A waitlisted player asked for a side but isn't on it: they're left out
    /// of its `player_count`, the match total *and* its `roster_preview`, even
    /// though their row carries the side's id. Guards the waitlist quietly
    /// filling a side (or showing on it) with people who aren't playing.
    #[test]
    fn a_waitlisted_player_is_not_counted_or_previewed_on_the_side_they_asked_for() {
        let waiting = MatchPlayerRecord {
            waitlisted_at: Some("2026-09-14T00:00:00Z".into()),
            ..player("waiting", Some("b"), None)
        };
        let players = [player("in", Some("b"), None), waiting];
        let write = recount_write(&stored(Some(2), &[("b", Some(2))]), &players).unwrap();
        assert_eq!(
            write.counts,
            RosterCounts {
                total_player_count: 1,
                side_player_counts: HashMap::from([("b".to_string(), 1)]),
            }
        );
        let (_, preview_b) = side_roster("b", &players);
        let previewed: Vec<&str> = preview_b.iter().map(|p| p.player_id.as_str()).collect();
        assert_eq!(previewed, ["in"]);
    }

    /// Every write that takes a spot (join, accept, move-in) builds its `ADD`
    /// here, so this guard is what keeps all three under the caps. Accepting
    /// an invite used to `ADD` with no cap condition at all, so an accept could
    /// take a match past its cap.
    #[test]
    fn taking_a_spot_is_guarded_on_every_cap() {
        let capped = take_slot_update(
            "agon",
            "m",
            &SlotCaps {
                side_id: Some("b".into()),
                side_max_players: Some(3),
                total_max_players: Some(6),
            },
        )
        .unwrap();
        assert_eq!(
            capped.update_expression(),
            "ADD total_player_count :one, sides.#sid.player_count :one"
        );
        assert_eq!(
            capped.condition_expression(),
            Some(
                "attribute_exists(#pk) \
                 AND (attribute_not_exists(total_player_count) OR total_player_count < :totalmax) \
                 AND (attribute_not_exists(sides.#sid.max_players) OR sides.#sid.player_count < :sidemax)"
            )
        );
        let values = capped.expression_attribute_values().unwrap();
        assert_eq!(values[":totalmax"], AttributeValue::N("6".into()));
        assert_eq!(values[":sidemax"], AttributeValue::N("3".into()));

        let uncapped = take_slot_update("agon", "m", &SlotCaps::default()).unwrap();
        assert_eq!(uncapped.update_expression(), "ADD total_player_count :one");
        assert_eq!(
            uncapped.condition_expression(),
            Some("attribute_exists(#pk)")
        );
    }

    /// After a failed cap guard, the recount's fresh counts decide between
    /// another try for a spot and the waitlist (or a move-in's "no free
    /// spot"), under the same caps the guard used: the match total, and the
    /// side's own when the spot is on a capped side.
    #[test]
    fn has_room_checks_the_match_and_the_sides_cap() {
        let counts = RosterCounts {
            total_player_count: 3,
            side_player_counts: HashMap::from([("a".to_string(), 2), ("b".to_string(), 1)]),
        };
        let caps = |side: Option<&str>, side_max: Option<u32>, total_max: Option<u32>| SlotCaps {
            side_id: side.map(Into::into),
            side_max_players: side_max,
            total_max_players: total_max,
        };
        assert!(caps(None, None, None).has_room(&counts), "uncapped");
        assert!(caps(None, None, Some(4)).has_room(&counts));
        assert!(!caps(None, None, Some(3)).has_room(&counts), "match full");
        assert!(caps(Some("b"), Some(2), Some(4)).has_room(&counts));
        assert!(
            !caps(Some("a"), Some(2), Some(4)).has_room(&counts),
            "side a full, though the match isn't"
        );
        assert!(
            !caps(Some("b"), Some(2), Some(3)).has_room(&counts),
            "side b has room, but the match doesn't"
        );
    }
}
