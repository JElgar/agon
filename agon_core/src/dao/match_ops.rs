//! Match operations: create (meta + sides + players in one transaction),
//! get aggregate, update meta, live-scoring score record, and player roster
//! writes.

use std::collections::HashMap;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, ItemBuilder, from_item, item_pk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{
    ConfirmedScoreRecord, HeaderPhotoRecord, LocationRecord, MatchFormatRecord, MatchPlayerRecord,
    MatchRecord, MatchScoreRecord, MatchSideRecord, PendingScoreRecord, SideRosterMemberRecord,
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
/// The two halves deliberately count different things. `player_count` is what
/// a side's `max_players` is enforced against, so it counts only players who
/// take a spot (`MatchPlayerRecord::occupies_slot`) — a pending or declined
/// invite holds none. `roster_preview` is a display of who's been placed on
/// the side, pending invitees included, so a feed card still shows who's been
/// asked.
///
/// `player_count`'s value here is only ever used to *seed* it (`create_match`,
/// which has no prior value to maintain a delta against). Every other write
/// that changes who occupies a slot maintains the stored counter directly via
/// an atomic `ADD` in its own transaction (`take_slot_update`/
/// `headcount_delta_update`) — never by recomputing and overwriting it, which
/// would race a concurrent counter update landing between this function's
/// read and whatever wrote it. This function itself stays a pure, sync
/// computation over an in-memory slice for exactly that reason: it has no way
/// to accidentally clobber a concurrent write.
fn side_roster(side_id: &str, players: &[MatchPlayerRecord]) -> (u32, Vec<SideRosterMemberRecord>) {
    let on_side: Vec<&MatchPlayerRecord> = players
        .iter()
        .filter(|p| p.side_id.as_deref() == Some(side_id))
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
/// who take a spot (`MatchPlayerRecord::occupies_slot`) across every side plus
/// unassigned — not every roster row. See `side_roster`'s doc comment: only
/// ever used to seed the counter, never to recompute it.
fn total_player_count(players: &[MatchPlayerRecord]) -> u64 {
    players.iter().filter(|p| p.occupies_slot()).count() as u64
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
/// but has no cap of its own — enforced by `Dao::join_match_tx`.
pub fn effective_max_players(sides: &[MatchSideRecord]) -> Option<u32> {
    sides
        .iter()
        .map(|s| s.max_players)
        .try_fold(0u32, |total, max| Some(total + max?))
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
        // Seed the atomic headcount every roster-changing write maintains
        // from here on via `ADD` — every player at creation who takes a spot,
        // side-assigned or not. A pending invitee doesn't count until they
        // accept.
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

    /// Update a match's mutable meta fields. Any `Some` field is written:
    /// `name`, `description`, `status`, `starts_at`, `location`, `format`
    /// (each overwritten wholesale, no clear case — mirrors `format`'s own
    /// doc comment), and the resolved `confirmed_score`/`pending_score`
    /// blobs (which do support clearing via `Some(None)`). `NotFound` if the
    /// match is absent.
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
        // Replace the location. `None` leaves it unchanged; `Some(value)`
        // overwrites. No "clear" case yet, same as `format` above.
        location: Option<LocationRecord>,
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
        if let Some(loc) = location {
            set.push("#loc = :loc".into());
            names.insert("#loc".into(), "location".into());
            values.insert(":loc".into(), to_attr(&loc)?);
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

    /// Set a player's role — mirrors `Dao::update_team_member_role` exactly.
    /// `role` is expected to be `"admin"` or `"player"` (never `"owner"`,
    /// which only ever moves via `transfer_match_ownership`) — the caller is
    /// responsible for that and every other business-rule check (caller may
    /// manage the match, target isn't the owner); this just writes the role.
    /// `NotFound` if the player doesn't exist.
    pub async fn update_match_player_role(
        &self,
        match_id: &str,
        player_id: &str,
        role: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Player(player_id.into()).to_string()))
            .update_expression("SET #role = :role")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#role", "role")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":role", s(role))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Err(DaoError::NotFound(format!(
                "player {player_id} on match {match_id}"
            ))),
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

    /// Join a match: insert `player` (fully built by the caller — `side_id`,
    /// `role`, `joined_via` already set) and atomically bump the roster
    /// counters it consumes, conditioned on the caps the caller already
    /// resolved from a just-read [`MatchAggregate`]: `side_max_players` for
    /// `player.side_id`'s side (ignored when `player.side_id` is `None`) and
    /// `total_max_players` for the match's overall
    /// [`effective_max_players`]. Also writes the joiner's own feed row (same
    /// as `create_match`/`accept_invitation_tx`) so the match shows up on
    /// their feed immediately.
    ///
    /// `Conflict` if a cap has been reached since the caller last read it (or
    /// the match has since disappeared) — the caller is expected to have
    /// already ruled out the common cases (full, already a player) from its
    /// own read, so this is a last-moment race guard, not the primary check.
    #[tracing::instrument(skip(self, player), fields(match_id, player_id = %player.player_id))]
    pub async fn join_match_tx(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        side_max_players: Option<u32>,
        total_max_players: Option<u32>,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<()> {
        let items = self.join_match_items(
            match_id,
            player,
            side_max_players,
            total_max_players,
            starts_at,
            now,
        )?;
        self.send_transact_items(items).await
    }

    /// Build the writes `join_match_tx` sends as one transaction — the roster
    /// put, the cap-guarded headcount `ADD`, and (if `player.user_id` is set)
    /// the joiner's own feed row — without sending them. Shared with
    /// `Dao::move_in_from_waitlist_tx`, which appends one more item (deleting
    /// the mover's waitlist entry) to the same list before sending, so a
    /// move-in either lands fully — on the roster and off the waitlist — or
    /// not at all, rather than the two as separate calls a partial failure
    /// could split apart.
    pub(super) fn join_match_items(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
        side_max_players: Option<u32>,
        total_max_players: Option<u32>,
        starts_at: &str,
        now: &str,
    ) -> DaoResult<Vec<TransactWriteItem>> {
        let put_player = Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let update_meta = take_slot_update(
            self.table(),
            match_id,
            player.side_id.as_deref(),
            side_max_players,
            total_max_players,
        )?;

        let mut items = vec![
            TransactWriteItem::builder().put(put_player).build(),
            TransactWriteItem::builder().update(update_meta).build(),
        ];

        if let Some(uid) = &player.user_id {
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
            let feed_put = Put::builder()
                .table_name(self.table())
                .set_item(Some(feed_item))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            items.push(TransactWriteItem::builder().put(feed_put).build());
        }

        Ok(items)
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

    /// Add an ad-hoc player directly onto the roster (an organiser's own add
    /// — no invitation, so it always takes a spot), atomically bumping
    /// `total_player_count`/the target side's `player_count` alongside the
    /// put. Uncapped — an organiser can always add players regardless of any
    /// side's cap, unlike `join_match_tx`'s guarded self-serve path.
    #[tracing::instrument(skip(self, player), fields(player_id = %player.player_id))]
    pub async fn add_match_player_tx(
        &self,
        match_id: &str,
        player: &MatchPlayerRecord,
    ) -> DaoResult<()> {
        let put_player = Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, player)?))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        let side_deltas: Vec<(String, i64)> = player
            .side_id
            .as_ref()
            .map(|sid| (sid.clone(), 1))
            .into_iter()
            .collect();
        let update = headcount_delta_update(self.table(), match_id, 1, &side_deltas)?
            .expect("adding a player is always a +1 total delta");

        self.send_transact_items(vec![
            TransactWriteItem::builder().put(put_player).build(),
            TransactWriteItem::builder().update(update).build(),
        ])
        .await
    }

    /// Move an existing player to a different side (or to/from unassigned),
    /// atomically adjusting `sides.<old>.player_count`/`sides.<new>.player_count`
    /// alongside the put — only if `updated` currently occupies a slot (a
    /// still-pending invitee's reassignment touches no counter, matching
    /// `MatchPlayerRecord::occupies_slot`). `total_player_count` never
    /// changes: it's the same player, just on a different side. Uncapped,
    /// same as `add_match_player_tx`.
    #[tracing::instrument(skip(self, updated), fields(player_id = %updated.player_id))]
    pub async fn reassign_match_player_tx(
        &self,
        match_id: &str,
        updated: &MatchPlayerRecord,
        old_side_id: Option<&str>,
    ) -> DaoResult<()> {
        let put_player = Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, updated)?))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut side_deltas = Vec::new();
        if updated.occupies_slot() {
            if let Some(old) = old_side_id {
                side_deltas.push((old.to_string(), -1));
            }
            if let Some(new) = &updated.side_id {
                side_deltas.push((new.clone(), 1));
            }
        }

        let mut items = vec![TransactWriteItem::builder().put(put_player).build()];
        if let Some(update) = headcount_delta_update(self.table(), match_id, 0, &side_deltas)? {
            items.push(TransactWriteItem::builder().update(update).build());
        }
        self.send_transact_items(items).await
    }

    /// Remove players from a match's roster entirely (not just unassign their
    /// side — see `reassign_match_player_tx` for that), atomically
    /// decrementing `total_player_count`/each affected side's `player_count`
    /// for however many of `removed` occupied a slot — a pending or declined
    /// invitee's removal touches no counter. One `TransactWriteItems` (a
    /// `Delete` per player plus, if any occupied a slot, one `Update`) rather
    /// than `put_match_player`'s old chunked `BatchWriteItem`: `BatchWriteItem`
    /// has no room for a conditional/counter write alongside the deletes.
    /// DynamoDB caps a transaction at 100 items, so more than 99 players
    /// removed in one call would need chunking — not handled here (fine for
    /// real team sizes, same caveat `create_match` already carries). A no-op
    /// for an empty `removed`.
    #[tracing::instrument(skip(self, removed))]
    pub async fn remove_match_players(
        &self,
        match_id: &str,
        removed: &[MatchPlayerRecord],
    ) -> DaoResult<()> {
        if removed.is_empty() {
            return Ok(());
        }

        let mut total_delta = 0i64;
        let mut side_deltas: Vec<(String, i64)> = Vec::new();
        let mut items = Vec::with_capacity(removed.len() + 1);
        for player in removed {
            let delete = Delete::builder()
                .table_name(self.table())
                .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
                .key(ATTR_SK, s(Sk::Player(player.player_id.clone()).to_string()))
                .build()
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            items.push(TransactWriteItem::builder().delete(delete).build());

            if player.occupies_slot() {
                total_delta -= 1;
                if let Some(sid) = &player.side_id {
                    side_deltas.push((sid.clone(), -1));
                }
            }
        }
        if let Some(update) =
            headcount_delta_update(self.table(), match_id, total_delta, &side_deltas)?
        {
            items.push(TransactWriteItem::builder().update(update).build());
        }
        self.send_transact_items(items).await
    }

    /// Recompute and store every side's cached `roster_preview` from the
    /// match's *current* player collection — a display cache, not a counter
    /// (see `add_match_player_tx`/`reassign_match_player_tx`/
    /// `remove_match_players`/`join_match_tx`/`Dao::accept_invitation_items`
    /// for how `total_player_count`/`sides.*.player_count` are actually
    /// maintained: an atomic `ADD` alongside whatever write changed who
    /// occupies a slot, never a recompute). Call after any write that can
    /// change who's been placed on a side or who they are — invitation
    /// acceptance (linking external → user) is the one roster-composition
    /// change that doesn't itself touch `roster_preview`, since it doesn't go
    /// through the DAO methods above.
    ///
    /// This *is* a plain, unconditional recompute-and-overwrite — but for
    /// `roster_preview` that's harmless even under a race: it's a display
    /// snapshot, not a value anything's correctness depends on, and the next
    /// roster-changing write recomputes it from the true roster again.
    ///
    /// One `GetItem` (meta, projected to just `sides` — to learn the current
    /// side ids; sides never get added/removed/reassigned an id after
    /// creation, but this doesn't assume that) + one `Query` (current
    /// players — still per-match, player ids aren't known ahead of the read)
    /// + one `UpdateItem` that sets every side's `roster_preview` by key in a
    /// single call, regardless of how many sides there are.
    #[tracing::instrument(skip(self))]
    pub async fn refresh_side_roster_previews(&self, match_id: &str) -> DaoResult<()> {
        let side_ids = self.match_side_ids(match_id).await?;
        if side_ids.is_empty() {
            return Ok(()); // Match gone, or (shouldn't happen) has no sides.
        }

        let player_prefix = Sk::player_prefix();
        let players = self
            .query_match_collection::<MatchPlayerRecord>(match_id, &player_prefix)
            .await?;

        let mut set_clauses = Vec::with_capacity(side_ids.len());
        let mut names = HashMap::with_capacity(side_ids.len());
        let mut values = HashMap::with_capacity(side_ids.len());
        for (i, side_id) in side_ids.iter().enumerate() {
            let (_, roster_preview) = side_roster(side_id, &players);
            let name_alias = format!("#s{i}");
            set_clauses.push(format!("sides.{name_alias}.roster_preview = :rp{i}"));
            names.insert(name_alias, side_id.clone());
            values.insert(format!(":rp{i}"), to_attr(&roster_preview)?);
        }

        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression(format!("SET {}", set_clauses.join(", ")))
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(Some(values))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// The current side ids on a match's meta record, without pulling the
    /// rest of it — just enough for [`refresh_side_roster_previews`] to know
    /// which `sides.<id>` paths to update. Empty if the match doesn't exist.
    async fn match_side_ids(&self, match_id: &str) -> DaoResult<Vec<String>> {
        #[derive(serde::Deserialize)]
        struct SidesOnly {
            sides: HashMap<String, MatchSideRecord>,
        }

        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .projection_expression("sides")
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        let Some(item) = out.item else {
            return Ok(Vec::new()); // Match gone.
        };
        let parsed: SidesOnly = from_item(item)?;
        Ok(parsed.sides.into_keys().collect())
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

/// The cap-guarded `ADD` for a write that takes a real spot on a match —
/// `total_player_count` always, plus `sides.<side_id>.player_count` when
/// `side_id` is given — conditioned on the match existing and, for whichever
/// of `total_max_players`/`side_max_players` is `Some`, there still being
/// room. One builder shared by every path that can push a match/side to (or
/// past) its cap, so they can never disagree about what "full" means:
/// `Dao::join_match_tx` (self-serve) and `Dao::accept_invitation_items`
/// (invite acceptance into a real spot — not onto the waitlist, which takes
/// no spot and so never calls this).
///
/// Unlike [`headcount_delta_update`], this is never called on its own: the
/// caller always pairs it, in the same transaction, with the `Put` that
/// actually adds the player — so a failed guard here fails the whole write,
/// and a successful one always has a real player row to match.
pub(super) fn take_slot_update(
    table: &str,
    match_id: &str,
    side_id: Option<&str>,
    side_max_players: Option<u32>,
    total_max_players: Option<u32>,
) -> DaoResult<Update> {
    let mut names: HashMap<String, String> = HashMap::new();
    let mut values: HashMap<String, AttributeValue> = HashMap::new();
    names.insert("#pk".into(), ATTR_PK.into());
    values.insert(":one".into(), AttributeValue::N("1".into()));

    // A single `ADD` section, comma-separated — DynamoDB rejects an
    // `UpdateExpression` with more than one `ADD` keyword.
    let mut add_clauses = vec!["total_player_count :one".to_string()];
    let mut conditions: Vec<String> = vec!["attribute_exists(#pk)".into()];
    if let Some(max) = total_max_players {
        conditions.push(
            "(attribute_not_exists(total_player_count) OR total_player_count < :totalmax)".into(),
        );
        values.insert(":totalmax".into(), AttributeValue::N(max.to_string()));
    }
    if let Some(side_id) = side_id {
        names.insert("#sid".into(), side_id.to_string());
        add_clauses.push("sides.#sid.player_count :one".to_string());
        if let Some(max) = side_max_players {
            conditions.push(
                "(attribute_not_exists(sides.#sid.max_players) OR sides.#sid.player_count < :sidemax)"
                    .into(),
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

/// The plain (uncapped) headcount `ADD` for a write that doesn't need a
/// capacity guard — an organiser adding/removing/reassigning players
/// directly always overrides any cap, exactly like today. `total_delta`
/// changes `total_player_count`; `side_deltas` changes each named side's
/// `player_count` by its paired delta. Deltas for the same side id are
/// summed and a net-zero result dropped, so a reassignment back onto the
/// same side (or any other delta that cancels out) never emits a clause for
/// it — DynamoDB rejects an `UpdateExpression` naming the same document path
/// twice.
///
/// Always paired, in the same transaction, with whatever `Put`/`Delete`
/// actually changed the roster — safe to leave unconditional itself (beyond
/// the match still existing) because that other item's own guard is what
/// makes the whole transaction — and so this delta — commit only when the
/// roster really changed the way the delta assumes.
///
/// `Ok(None)` if every delta is zero (nothing to write) — `total_delta == 0`
/// and every side delta cancels out.
fn headcount_delta_update(
    table: &str,
    match_id: &str,
    total_delta: i64,
    side_deltas: &[(String, i64)],
) -> DaoResult<Option<Update>> {
    let mut merged: HashMap<String, i64> = HashMap::new();
    for (side_id, delta) in side_deltas {
        *merged.entry(side_id.clone()).or_insert(0) += delta;
    }
    merged.retain(|_, delta| *delta != 0);

    if total_delta == 0 && merged.is_empty() {
        return Ok(None);
    }

    let mut names: HashMap<String, String> = HashMap::new();
    let mut values: HashMap<String, AttributeValue> = HashMap::new();
    let mut add_clauses = Vec::new();
    if total_delta != 0 {
        add_clauses.push("total_player_count :total".to_string());
        values.insert(":total".into(), AttributeValue::N(total_delta.to_string()));
    }
    // Sorted so the expression (and so the test asserting it) is deterministic
    // — a `HashMap`'s iteration order isn't.
    let mut side_ids: Vec<&String> = merged.keys().collect();
    side_ids.sort();
    for (i, side_id) in side_ids.into_iter().enumerate() {
        let alias = format!("#s{i}");
        add_clauses.push(format!("sides.{alias}.player_count :d{i}"));
        names.insert(alias, side_id.clone());
        values.insert(
            format!(":d{i}"),
            AttributeValue::N(merged[side_id].to_string()),
        );
    }
    names.insert("#pk".into(), ATTR_PK.into());

    Update::builder()
        .table_name(table)
        .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
        .key(ATTR_SK, s(Sk::Meta.to_string()))
        .update_expression(format!("ADD {}", add_clauses.join(", ")))
        .condition_expression("attribute_exists(#pk)")
        .set_expression_attribute_names(Some(names))
        .set_expression_attribute_values(Some(values))
        .build()
        .map(Some)
        .map_err(|e| DaoError::Dynamo(e.to_string()))
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

    /// Deltas for the same side sum, and a net-zero result (a reassignment
    /// back onto the side it started on, or any other cancelling pair) drops
    /// that side's clause entirely — DynamoDB rejects an `UpdateExpression`
    /// naming the same document path twice, so this is load-bearing, not
    /// just tidiness.
    #[test]
    fn headcount_delta_update_merges_and_drops_net_zero_sides() {
        let update = headcount_delta_update(
            "table",
            "m1",
            0,
            &[("a".into(), -1), ("a".into(), 1), ("b".into(), 2)],
        )
        .unwrap()
        .expect("side b still has a net delta");
        let expr = update.update_expression();
        assert!(!expr.contains("total_player_count"), "total delta was 0");
        // Side "a"'s +1/-1 cancelled — only "b" should appear.
        assert_eq!(expr.matches("player_count").count(), 1);
    }

    /// Every delta cancelling out (or all zero to begin with) means nothing
    /// to write — the caller skips the `Update` transact item entirely rather
    /// than sending a no-op.
    #[test]
    fn headcount_delta_update_is_none_when_everything_cancels() {
        assert!(
            headcount_delta_update("table", "m1", 0, &[("a".into(), 1), ("a".into(), -1)])
                .unwrap()
                .is_none()
        );
        assert!(
            headcount_delta_update("table", "m1", 0, &[])
                .unwrap()
                .is_none()
        );
    }

    /// A plain add/remove (no side involved) only ever touches
    /// `total_player_count`.
    #[test]
    fn headcount_delta_update_total_only() {
        let update = headcount_delta_update("table", "m1", -1, &[])
            .unwrap()
            .expect("nonzero total delta");
        let expr = update.update_expression();
        assert_eq!(expr, "ADD total_player_count :total");
    }
}
