//! Match operations: create (meta + sides + players in one transaction),
//! get aggregate, update meta, live-scoring score record, and player roster
//! writes.

use std::collections::HashMap;

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem};

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
fn side_roster(side_id: &str, players: &[MatchPlayerRecord]) -> (u32, Vec<SideRosterMemberRecord>) {
    let on_side: Vec<&MatchPlayerRecord> = players
        .iter()
        .filter(|p| p.side_id.as_deref() == Some(side_id))
        .collect();
    let player_count = on_side.len() as u32;
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

/// A match plus its sides and players, assembled from one collection query.
/// Excludes the live-scoring score record, likes and comments (fetched separately).
#[derive(Debug)]
pub struct MatchAggregate {
    pub match_: MatchRecord,
    pub sides: Vec<MatchSideRecord>,
    pub players: Vec<MatchPlayerRecord>,
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

    /// Recompute and store every side's `player_count`/`roster_preview` from
    /// the match's *current* player collection. Call after any write that can
    /// change a side's composition or a player's identity within it —
    /// invitation acceptance (linking external → user) and `put_match_player`
    /// (roster reconciliation / late adds). `create_match` doesn't need this:
    /// it computes the same thing inline, from the roster it already has in
    /// memory, within the same transaction.
    ///
    /// One `GetItem` (meta, projected to just `sides` — to learn the current
    /// side ids; sides never get added/removed/reassigned an id after
    /// creation, but this doesn't assume that) + one `Query` (current
    /// players — still per-match, player ids aren't known ahead of the read)
    /// + one `UpdateItem` that sets every side's `player_count`/
    /// `roster_preview` by key in a single call, regardless of how many
    /// sides there are. Runs on the rarer roster-mutating write paths, never
    /// on a feed read — the whole point is for the feed to *avoid*
    /// re-deriving this live.
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

        let mut set_clauses = Vec::with_capacity(side_ids.len() * 2);
        let mut names = HashMap::with_capacity(side_ids.len());
        let mut values = HashMap::with_capacity(side_ids.len() * 2);
        for (i, side_id) in side_ids.iter().enumerate() {
            let (player_count, roster_preview) = side_roster(side_id, &players);
            let name_alias = format!("#s{i}");
            set_clauses.push(format!(
                "sides.{name_alias}.player_count = :pc{i}, sides.{name_alias}.roster_preview = :rp{i}"
            ));
            names.insert(name_alias, side_id.clone());
            values.insert(format!(":pc{i}"), to_attr(&player_count)?);
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

fn is_update_conditional_failure(err: &SdkError<UpdateItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}
