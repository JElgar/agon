//! Rating storage: a player's per-ladder rating, their rating history, what
//! each match contributed to it, and the ladders they have opted in to.
//!
//! | What | Where | Why |
//! |---|---|---|
//! | opt-in | `rating_opt_ins.<ladder>` on `USER#<uid>`/`#PROFILE` | the consent every path into a ranked game checks |
//! | current rating | `ratings.<ladder>` on `USER#<uid>`/`#PROFILE` | rides the point read profiles, feed and search hydration already do |
//! | history | `USER#<uid>` / `RATING#<ladder>#<played_at>#<mid>` | replay source *and* the rating-over-time chart, from one write |
//! | contribution | `MATCH#<mid>` / `RATINGCONTRIB#<userId>` | makes re-rating idempotent, and is what the UI reads to show "+18" |
//!
//! The last three are written together, in one transaction per player per
//! match, and read apart. Nothing calls any of this yet: the callers are the
//! planned follow-ups — ranked games and opt-in enforcement at the API,
//! rating a confirmed match in the worker, and the `RepairRatings` replay.
//!
//! ## Players only
//!
//! Every operation takes a user id, because teams are not rated. That is a
//! product decision rather than a gap, so there is no owner abstraction
//! waiting for a second kind. If teams are ever rated, two things carry over:
//! a team's rating needs history exactly like a player's (a rated entity with
//! no history has no replay source, and so no repair path), and its map would
//! hang off `TEAM#<tid>`/`#META`, not `#PROFILE`.
//!
//! ## The change-detection protocol (read this before writing the handler)
//!
//! `Dao::reconcile_match_contribution` does its own read, computes its own
//! delta and skips the write when nothing changed. The rating equivalent
//! deliberately does **not**: [`Dao::apply_rating_contribution`] writes what
//! it is told. The reason is that "has this match's rating effect changed?"
//! is not answerable from one participant's record.
//!
//! The trap is worth spelling out, because the obvious implementation is
//! wrong in a way that silently double-counts. Suppose match A is rated,
//! then match B, then A's `#META` is rewritten by a like and redelivered. A
//! handler that re-rates A against the player's *current* rating computes a
//! different movement (it now starts from B's output), sees it differ from
//! the stored contribution, and applies it — counting A twice.
//!
//! The right test re-rates A from the ratings the participants carried *into*
//! A, which is precisely what [`RatingContributionRecord::movement`]'s
//! `mu_before`/`sigma_before` preserve. So, across the whole match and before
//! anything is written:
//!
//! 1. [`Dao::list_rating_contributions`] for the match.
//! 2. Rate the match's *current* roster and `winner_side_id` with
//!    `rating::rate_sides`, starting each player from the `before` on their
//!    stored contribution if they have one, and from their current rating
//!    only if they don't.
//! 3. For each player with a stored contribution, compare with
//!    [`RatingContributionRecord::has_same_effect_as`]. Equal means genuinely
//!    nothing changed for them. Different means the match itself changed
//!    (re-score, roster edit, sport or time edit), which is a repair — never
//!    an incremental rewrite of the stored contribution.
//! 4. A player with *no* stored contribution is applied, even when step 3
//!    found a change for somebody else in the same delivery. A repair replays
//!    stored history, so it can correct a contribution but never create a
//!    missing one: skip this player now and they are never rated for the
//!    match. The same rule is what completes a partially-applied match,
//!    since each player's apply is its own transaction.
//! 5. A stored contribution the match no longer implies (cancelled, player
//!    removed) is withdrawn with [`Dao::withdraw_rating_contribution`], and
//!    the rating it leaves stale is a repair's job.
//!
//! That is why the contribution stores each player's incoming rating and
//! `side_id` at all.

use std::collections::HashMap;

use aws_sdk_dynamodb::operation::update_item::builders::UpdateItemFluentBuilder;
use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};
use serde::Deserialize;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{RatingContributionRecord, RatingHistoryRecord, RatingRecord};

/// Type tag for the per-match rating-contribution item.
pub const TYPE_RATING_CONTRIBUTION: &str = "rating_contribution";
/// Type tag for a rating-history item.
pub const TYPE_RATING_HISTORY: &str = "rating_history";

/// The profile-item attribute holding the per-ladder rating map. Must stay in
/// step with the field name `UserRecord::ratings` — a rename there without a
/// change here compiles cleanly and then writes ratings into an attribute
/// nothing reads. Pinned by
/// `tests::every_attribute_an_expression_names_exists_on_its_record`.
const ATTR_RATINGS: &str = "ratings";

/// The profile-item attribute holding the per-ladder opt-in map. The same
/// hazard as [`ATTR_RATINGS`], pinned by the same test.
const ATTR_RATING_OPT_INS: &str = "rating_opt_ins";

impl RatingContributionRecord {
    /// The history entry this contribution implies for its player.
    ///
    /// Derived rather than passed in alongside it: the two records overlap in
    /// everything but `match_id`, and a caller assembling both by hand could
    /// let them disagree — which would put a history item in the chart that
    /// no contribution can ever withdraw. Guarded by
    /// `tests::the_history_entry_mirrors_the_contribution`.
    #[must_use]
    pub fn history_entry(&self, match_id: &str) -> RatingHistoryRecord {
        RatingHistoryRecord {
            ladder: self.ladder.clone(),
            match_id: match_id.to_string(),
            played_at: self.played_at.clone(),
            movement: self.movement,
            applied_at: self.applied_at.clone(),
        }
    }

    /// The sort key of that history entry, in the player's partition.
    #[must_use]
    pub fn history_sk(&self, match_id: &str) -> Sk {
        Sk::Rating {
            ladder: self.ladder.clone(),
            played_at: self.played_at.clone(),
            match_id: match_id.to_string(),
        }
    }
}

/// Just the `ratings` map off a profile item.
///
/// A projection rather than `Dao::get_user`: not for the RCUs (DynamoDB
/// charges for the whole item either way) but so that reading a rating
/// depends on nothing but the `ratings` attribute's shape. The rating
/// pipeline reads this on every rated match, and deserializing the whole
/// profile would let an unrelated field that fails to parse — the class of
/// bug `records::tests::sparse_cricket_stats_deserializes` exists for — stop
/// ratings as well.
#[derive(Debug, Deserialize)]
struct RatingsProjection {
    #[serde(default)]
    ratings: HashMap<String, RatingRecord>,
}

impl Dao {
    /// Every ladder this player is rated on.
    ///
    /// A user that does not exist and one that exists but has never been
    /// rated both come back as an empty map. That conflation is deliberate:
    /// the planned caller, the rating pipeline, reads this only to write
    /// next, and every write below first runs a step guarded on
    /// `attribute_exists(PK)`. A missing user is caught there — with a
    /// `NotFound` naming them — rather than here, where distinguishing the two
    /// would cost a second read on every rated match.
    #[tracing::instrument(skip(self))]
    pub async fn get_ratings(&self, user_id: &str) -> DaoResult<HashMap<String, RatingRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Profile.to_string()))
            .projection_expression("#ratings")
            .expression_attribute_names("#ratings", ATTR_RATINGS)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        match out.item {
            Some(item) => Ok(from_item::<RatingsProjection>(item)?.ratings),
            None => Ok(HashMap::new()),
        }
    }

    /// This player's rating on one ladder, or `None` if they have never been
    /// rated on it.
    ///
    /// `None` is not the same as "1500 with maximum uncertainty" *to this
    /// layer*, even though the engine treats them identically
    /// (`rating::group_by_side` defaults an absent player). The difference
    /// matters one level up: `None` is what makes the first write's
    /// `attribute_not_exists` guard meaningful, so a rating cannot be created
    /// twice by two racing first matches.
    #[tracing::instrument(skip(self))]
    pub async fn get_rating(&self, user_id: &str, ladder: &str) -> DaoResult<Option<RatingRecord>> {
        Ok(self.get_ratings(user_id).await?.remove(ladder))
    }

    /// Set one ladder's rating, guarded on the value the caller read.
    ///
    /// Standalone counterpart to the three-item write in
    /// [`Self::apply_rating_contribution`], for the paths that move a rating
    /// without a match behind it: σ-inflation on inactivity, and the end of a
    /// repair replay, which has to leave the right value behind even when
    /// none of its per-match writes did.
    ///
    /// `expected` is the rating read before computing the new one, or `None`
    /// if there was none. Passing the wrong one is a `Conflict`, not a silent
    /// overwrite: two writers on the same ladder at the same instant would
    /// otherwise both compute from the same base and the loser would vanish.
    #[tracing::instrument(skip(self, rating, expected))]
    pub async fn put_rating(
        &self,
        user_id: &str,
        ladder: &str,
        rating: &RatingRecord,
        expected: Option<&RatingRecord>,
    ) -> DaoResult<()> {
        self.ensure_profile_map(user_id, ATTR_RATINGS).await?;
        let update = self.rating_update(user_id, ladder, rating, expected)?;
        self.run_guarded(
            vec![TransactWriteItem::builder().update(update).build()],
            || {
                DaoError::Conflict(format!(
                    "rating for user {user_id} on ladder {ladder} changed concurrently"
                ))
            },
        )
        .await
    }

    /// Record that a player has opted in to being rated on `ladder`, as of
    /// `now` (RFC 3339). `true` if this call opted them in, `false` if they
    /// already were.
    ///
    /// Idempotent in the way consent needs to be: the first opt-in's
    /// timestamp is kept for good, and a repeat — the box ticked again on a
    /// second ranked game, or a retried request — changes nothing. That is a
    /// condition on the write (`attribute_not_exists` on the ladder's path),
    /// not a read followed by a write, so two concurrent opt-ins cannot both
    /// report `true`.
    ///
    /// `ladder` must come from `rating::ladder_for_tag` on the match's sport,
    /// never from request input — it becomes a map key on the profile.
    ///
    /// `NotFound` if the profile does not exist, and neither round trip can
    /// create one: both are conditioned on `attribute_exists(PK)`, because an
    /// `UpdateItem` against a missing key *creates* the item.
    ///
    /// A failed condition on the second write is read as "already opted in",
    /// and that relies on something worth stating. The ensure step has just
    /// proved the profile exists and nothing deletes a `#PROFILE` item, so the
    /// ladder already being set is the only way left for the condition to
    /// fail. If account deletion is ever added, this has to learn to tell the
    /// two apart (`ReturnValuesOnConditionCheckFailure` would do it).
    ///
    /// There is deliberately no counterpart: opting in is not reversible.
    #[tracing::instrument(skip(self))]
    pub async fn opt_in_to_rating(
        &self,
        user_id: &str,
        ladder: &str,
        now: &str,
    ) -> DaoResult<bool> {
        self.ensure_profile_map(user_id, ATTR_RATING_OPT_INS)
            .await?;
        match self.opt_in_request(user_id, ladder, now).send().await {
            Ok(_) => Ok(true),
            Err(e) if is_update_conditional_failure(&e) => Ok(false),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Apply one player's rating movement from one match: the new rating, the
    /// contribution that produced it and the history entry recording it, as a
    /// single transaction.
    ///
    /// All three or none, and that is the invariant the whole idempotence
    /// story rests on. A crash between "moved the rating" and "wrote the
    /// contribution" would leave a movement that nothing records, so
    /// redelivery would find no contribution, treat the match as unrated and
    /// apply it a second time.
    ///
    /// Deliberately *not* a reconciler in the shape of
    /// `Dao::reconcile_match_contribution`: it does no read of its own and
    /// has no "nothing changed, skip it" branch. Deciding whether anything
    /// changed needs the whole match's contribution collection, not this one
    /// player's — see the module doc, which spells out the double-counting
    /// bug that the obvious per-player version walks into.
    ///
    /// `stored` / `stored_rating` are the values the caller read before
    /// computing, and become the optimistic-lock guards. `Ok(())` means the
    /// state the caller computed against was still current when the write
    /// landed.
    ///
    /// Re-applying a *changed* contribution through this is only ever
    /// provisional: the rating moves from the stored `before`, but every later
    /// match on the ladder was rated from the old result, and only a replay
    /// corrects those — which is why the protocol treats a change as a repair
    /// rather than an apply.
    #[tracing::instrument(skip(self, contribution, stored, rating, stored_rating))]
    pub async fn apply_rating_contribution(
        &self,
        match_id: &str,
        contribution: &RatingContributionRecord,
        stored: Option<&RatingContributionRecord>,
        rating: &RatingRecord,
        stored_rating: Option<&RatingRecord>,
    ) -> DaoResult<()> {
        self.ensure_profile_map(&contribution.user_id, ATTR_RATINGS)
            .await?;
        let tx =
            self.rating_contribution_items(match_id, contribution, stored, rating, stored_rating)?;
        self.run_guarded(tx, || {
            DaoError::Conflict(format!(
                "rating state for user {} from match {match_id} changed concurrently",
                contribution.user_id
            ))
        })
        .await
    }

    /// Back a match's contribution out of one player: delete the contribution
    /// item and its history entry, in one transaction.
    ///
    /// For a match that stopped being rateable — cancelled, or the player
    /// dropped from the roster.
    ///
    /// It deliberately does **not** touch the stored rating, which is the
    /// obvious thing to expect it to do and is not possible: a Weng-Lin
    /// update is not invertible, so "subtract this match" has no closed form
    /// — the only way back is to replay the ladder's history without it,
    /// which is `RepairRatings`' job. Removing the two items first is what
    /// makes that replay produce the right answer, so this is the step before
    /// the repair, not a substitute for it. Until the repair runs, the
    /// player's `ratings.<ladder>` still includes this match's effect; the
    /// history no longer explains it.
    #[tracing::instrument(skip(self, stored))]
    pub async fn withdraw_rating_contribution(
        &self,
        match_id: &str,
        stored: &RatingContributionRecord,
    ) -> DaoResult<()> {
        let tx = self.withdrawal_items(match_id, stored)?;
        self.run_guarded(tx, || {
            DaoError::Conflict(format!(
                "rating contribution for user {} from match {match_id} changed concurrently",
                stored.user_id
            ))
        })
        .await
    }

    /// This match's rating contribution for one player, if it has been rated
    /// for them.
    #[tracing::instrument(skip(self))]
    pub async fn get_rating_contribution(
        &self,
        match_id: &str,
        user_id: &str,
    ) -> DaoResult<Option<RatingContributionRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(
                ATTR_SK,
                s(Sk::RatingContribution(user_id.into()).to_string()),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        out.item.map(from_item).transpose()
    }

    /// Every rating contribution stored for a match.
    ///
    /// Returns whole records where the stats analogue
    /// (`list_stat_contribution_user_ids`) returns bare ids, because the
    /// records *are* the input the change-detection protocol in this module's
    /// header runs on. Fetching ids and then re-reading each record would be
    /// a round trip per player for data the query already carried.
    ///
    /// Unpaginated, like its stats counterpart: this is one item per player
    /// of one match, which is bounded by the roster.
    #[tracing::instrument(skip(self))]
    pub async fn list_rating_contributions(
        &self,
        match_id: &str,
    ) -> DaoResult<Vec<RatingContributionRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::rating_contribution_prefix()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        out.items
            .unwrap_or_default()
            .into_iter()
            .map(from_item)
            .collect()
    }

    /// One ladder's rating history for a player, **oldest first**, from
    /// `from` (a `played_at`, inclusive) onwards if given.
    ///
    /// Ascending, unlike `list_feed`'s newest-first, and there is no
    /// direction flag: both planned readers of this collection want played
    /// order. A replay walks forwards by definition, and the rating-over-time
    /// chart plots left to right. A newest-first mode would be a second sort
    /// direction to keep correct for the sake of a reversal the caller can do
    /// on a page it already holds.
    ///
    /// Paginated because it is the replay source. A three-year-old account's
    /// squash history is thousands of items, and the planned `RepairRatings`
    /// is a chunked workflow precisely so it never has to hold all of them.
    #[tracing::instrument(skip(self))]
    pub async fn list_rating_history(
        &self,
        user_id: &str,
        ladder: &str,
        from: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<RatingHistoryRecord>> {
        // `BETWEEN` rather than `begins_with`, because DynamoDB allows only
        // one sort-key condition and the from-a-point form needs a lower bound
        // *and* a ceiling that stops at this ladder — see
        // `Sk::rating_history_end`.
        let low = match from {
            Some(played_at) => Sk::rating_history_from(ladder, played_at),
            None => Sk::rating_prefix(ladder),
        };
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND SK BETWEEN :lo AND :hi")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
                .expression_attribute_values(":lo", s(low))
                .expression_attribute_values(":hi", s(Sk::rating_history_end(ladder)))
                .scan_index_forward(true),
            cursor,
            limit,
        )
        .await
    }

    /// The transaction [`Self::apply_rating_contribution`] runs, built apart
    /// from sending it so its shape is unit-testable — the fourth item above
    /// all, whose presence is the whole difference between moving a
    /// rescheduled match's history and duplicating it.
    fn rating_contribution_items(
        &self,
        match_id: &str,
        contribution: &RatingContributionRecord,
        stored: Option<&RatingContributionRecord>,
        rating: &RatingRecord,
        stored_rating: Option<&RatingRecord>,
    ) -> DaoResult<Vec<TransactWriteItem>> {
        let user_pk = Pk::User(contribution.user_id.clone());
        let mut tx: Vec<TransactWriteItem> = Vec::with_capacity(4);

        // 1. The contribution itself, guarded on what we read.
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::RatingContribution(contribution.user_id.clone()),
            TYPE_RATING_CONTRIBUTION,
            contribution,
        )?;
        let mut put = Put::builder().table_name(self.table()).set_item(Some(item));
        put = match stored {
            None => put
                .condition_expression("attribute_not_exists(#pk)")
                .expression_attribute_names("#pk", ATTR_PK),
            Some(previous) => guard_contribution(put, previous)?,
        };
        tx.push(
            TransactWriteItem::builder()
                .put(put.build().map_err(|e| DaoError::Dynamo(e.to_string()))?)
                .build(),
        );

        // 2. The player's rating on this ladder.
        tx.push(
            TransactWriteItem::builder()
                .update(self.rating_update(
                    &contribution.user_id,
                    &contribution.ladder,
                    rating,
                    stored_rating,
                )?)
                .build(),
        );

        // 3. The history entry. Unguarded, because its key is derived from
        //    (ladder, played_at, match) — re-applying the same match rewrites
        //    the same row rather than appending a second one, so an overwrite
        //    is a refresh. (Same idempotent-on-the-sort-key property the feed
        //    fan-out relies on.)
        let history_sk = contribution.history_sk(match_id);
        tx.push(
            TransactWriteItem::builder()
                .put(
                    Put::builder()
                        .table_name(self.table())
                        .set_item(Some(to_item(
                            &user_pk,
                            &history_sk,
                            TYPE_RATING_HISTORY,
                            &contribution.history_entry(match_id),
                        )?))
                        .build()
                        .map_err(|e| DaoError::Dynamo(e.to_string()))?,
                )
                .build(),
        );

        // 4. If the match moved ladders (its sport was edited) or was
        //    rescheduled, the history entry just written is at a *different*
        //    key from the one already stored, and the old row would otherwise
        //    survive as a duplicate — the chart would show the match twice
        //    and a replay would count it twice. Back it out in the same
        //    transaction. This is the rating analogue of the "sport changed,
        //    move the counters between sports" branch in
        //    `reconcile_match_contribution`.
        if let Some(previous) = stored {
            let previous_sk = previous.history_sk(match_id);
            if previous_sk != history_sk {
                tx.push(
                    TransactWriteItem::builder()
                        .delete(
                            Delete::builder()
                                .table_name(self.table())
                                .key(ATTR_PK, s(user_pk.to_string()))
                                .key(ATTR_SK, s(previous_sk.to_string()))
                                .build()
                                .map_err(|e| DaoError::Dynamo(e.to_string()))?,
                        )
                        .build(),
                );
            }
        }

        Ok(tx)
    }

    /// The transaction [`Self::withdraw_rating_contribution`] runs, built
    /// apart from sending it for the same reason as
    /// [`Self::rating_contribution_items`].
    fn withdrawal_items(
        &self,
        match_id: &str,
        stored: &RatingContributionRecord,
    ) -> DaoResult<Vec<TransactWriteItem>> {
        let delete = guard_contribution(
            Delete::builder()
                .table_name(self.table())
                .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
                .key(
                    ATTR_SK,
                    s(Sk::RatingContribution(stored.user_id.clone()).to_string()),
                ),
            stored,
        )?
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // Unguarded on purpose. The guarded contribution delete above already
        // serialises concurrent withdrawals, and the history row is only ever
        // written and deleted in the same transaction as the contribution, so
        // a condition here would add nothing. What it would add is a way to
        // get stuck: deleting a missing item is a no-op, but a guarded delete
        // of a history row that is missing for any reason would fail every
        // attempt, leaving the contribution impossible to withdraw.
        let delete_history = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(stored.user_id.clone()).to_string()))
            .key(ATTR_SK, s(stored.history_sk(match_id).to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        Ok(vec![
            TransactWriteItem::builder().delete(delete).build(),
            TransactWriteItem::builder().delete(delete_history).build(),
        ])
    }

    /// The `SET ratings.<ladder> = :rating` update, guarded on `expected` and
    /// on the profile still existing.
    ///
    /// Shared by [`Self::put_rating`] and [`Self::apply_rating_contribution`]
    /// so the two can never guard differently — the second one's guard is the
    /// only thing standing between two concurrently-confirmed matches on the
    /// same ladder and a lost update.
    ///
    /// The guard compares the whole `RatingRecord` map in one condition,
    /// which DynamoDB supports directly on map-typed attributes (the trick
    /// `reconcile_match_contribution` uses for its `counters`). It is exact
    /// rather than approximate on the two floats because the value being
    /// compared was serialized by this same code from a value read back
    /// unchanged — see `records::tests::rating_record_round_trips_mu_and_sigma_exactly`.
    fn rating_update(
        &self,
        user_id: &str,
        ladder: &str,
        rating: &RatingRecord,
        expected: Option<&RatingRecord>,
    ) -> DaoResult<Update> {
        let mut b = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Profile.to_string()))
            .update_expression("SET #ratings.#ladder = :rating")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#ratings", ATTR_RATINGS)
            // A ladder is user-invisible data but still an arbitrary string,
            // and `#name` placeholders are the only way to be sure it never
            // collides with a DynamoDB reserved word.
            .expression_attribute_names("#ladder", ladder)
            .expression_attribute_values(":rating", serde_dynamo::to_attribute_value(rating)?);
        b = match expected {
            None => b.condition_expression(
                "attribute_exists(#pk) AND attribute_not_exists(#ratings.#ladder)",
            ),
            Some(previous) => b
                .condition_expression("attribute_exists(#pk) AND #ratings.#ladder = :expected")
                .expression_attribute_values(
                    ":expected",
                    serde_dynamo::to_attribute_value(previous)?,
                ),
        };
        b.build().map_err(|e| DaoError::Dynamo(e.to_string()))
    }

    /// The conditional `SET rating_opt_ins.<ladder> = :opted_in_at` behind
    /// [`Self::opt_in_to_rating`], built apart from sending it so the
    /// expressions are unit-testable.
    ///
    /// It sets the one ladder's path and nothing else. A `SET` of the whole
    /// map would be the same line shorter and would erase every other
    /// ladder's opt-in.
    fn opt_in_request(&self, user_id: &str, ladder: &str, now: &str) -> UpdateItemFluentBuilder {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Profile.to_string()))
            .update_expression("SET #opt_ins.#ladder = :opted_in_at")
            .condition_expression(
                "attribute_exists(#pk) AND attribute_not_exists(#opt_ins.#ladder)",
            )
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#opt_ins", ATTR_RATING_OPT_INS)
            // Same reserved-word reasoning as `rating_update`'s `#ladder`.
            .expression_attribute_names("#ladder", ladder)
            .expression_attribute_values(":opted_in_at", s(now))
    }

    /// Make sure the profile item has a map at `attr` (`ratings` or
    /// `rating_opt_ins`), so that a nested `SET <attr>.#ladder = …` has
    /// somewhere to resolve into.
    ///
    /// DynamoDB cannot `SET` a path under a map that does not exist, and
    /// every profile written before these fields shipped lacks both. A
    /// separate round trip rather than a clause in the same expression, for
    /// the same reason `ensure_stats_sport` is one: `SET m = if_not_exists(m,
    /// :empty)` and `SET m.#ladder = :v` overlap on one document path, which
    /// DynamoDB rejects outright — and splitting them across two items of one
    /// transaction is not possible either, since a transaction may not touch
    /// the same item twice. It only runs on a rating write or an opt-in, both
    /// rare per player.
    ///
    /// Unlike `ensure_stats_sport` this is conditional on the item existing,
    /// and the divergence is deliberate. An `UpdateItem` against a missing key
    /// *creates* it, so without the guard a write naming a nonexistent user
    /// would leave a stub holding nothing but `PK`/`SK` and an empty map — a
    /// profile-shaped row that every read would then fail to deserialize.
    ///
    /// One function for both maps because it is the same problem twice.
    /// `attr` is `'static` because it is only ever one of this module's
    /// constants, never caller input.
    async fn ensure_profile_map(&self, user_id: &str, attr: &'static str) -> DaoResult<()> {
        match self.ensure_profile_map_request(user_id, attr).send().await {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("user {user_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// The request [`Self::ensure_profile_map`] sends, built apart so the
    /// expressions are unit-testable.
    fn ensure_profile_map_request(
        &self,
        user_id: &str,
        attr: &'static str,
    ) -> UpdateItemFluentBuilder {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Profile.to_string()))
            .update_expression("SET #map = if_not_exists(#map, :empty)")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#map", attr)
            .expression_attribute_values(":empty", AttributeValue::M(HashMap::new()))
    }

    /// Run a transaction, mapping a failed condition guard to a `Conflict`
    /// carrying `on_conflict`'s message.
    ///
    /// A `Conflict` here is not an error the caller should swallow: it means
    /// another writer moved the state between the caller's read and this
    /// write, so the work must be retried and recomputed from fresh state.
    /// Every rating write will be driven by an at-least-once stream event, so
    /// "fail and be redelivered" is a complete recovery strategy.
    async fn run_guarded(
        &self,
        tx: Vec<TransactWriteItem>,
        on_conflict: impl FnOnce() -> DaoError,
    ) -> DaoResult<()> {
        match self
            .client
            .transact_write_items()
            .set_transact_items(Some(tx))
            .send()
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(on_conflict()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

/// Constrain a contribution `Put`/`Delete` to the value we read (optimistic
/// lock).
///
/// Compares exactly the fields [`RatingContributionRecord::has_same_effect_as`]
/// does, and that pairing is deliberate: the handler decides "nothing changed"
/// with that method, so the write's guard has to agree with it field for
/// field, or a race could slip past the guard while the handler believed the
/// state was the one it read. `applied_at` is excluded from both — it is a
/// fresh wall clock on every delivery, so guarding on it would fail every
/// legitimate redelivery. Pinned by
/// `tests::the_contribution_guard_compares_the_effect_and_not_applied_at`.
fn guard_contribution<B: GuardBuilder>(b: B, previous: &RatingContributionRecord) -> DaoResult<B> {
    // `#name` placeholders throughout rather than the bare attribute names
    // `stats::guard` uses. These names (`ladder`, `movement`, …) are ours to
    // choose and DynamoDB's reserved-word list is long enough that checking
    // it by eye on every new field is a worse bet than always escaping.
    Ok(b.condition_expression(
        "#user_id = :user AND #ladder = :ladder AND #side_id = :side \
         AND #played_at = :played AND #movement = :movement",
    )
    .expression_attribute_names("#user_id", "user_id")
    .expression_attribute_names("#ladder", "ladder")
    .expression_attribute_names("#side_id", "side_id")
    .expression_attribute_names("#played_at", "played_at")
    .expression_attribute_names("#movement", "movement")
    .expression_attribute_values(":user", s(&previous.user_id))
    .expression_attribute_values(":ladder", s(&previous.ladder))
    .expression_attribute_values(":side", s(&previous.side_id))
    .expression_attribute_values(":played", s(&previous.played_at))
    .expression_attribute_values(
        ":movement",
        serde_dynamo::to_attribute_value(previous.movement)?,
    ))
}

/// The subset of `Put`/`Delete` builder methods [`guard_contribution`] needs
/// — lets one function guard either builder instead of duplicating it
/// per-variant. Deliberately a second, private copy of the trait `stats.rs`
/// declares for the same purpose rather than a shared one: the two guard
/// different records and share nothing but the method names, and hoisting it
/// would make `stats` and `rating` co-vary for no reason.
trait GuardBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self;
    fn expression_attribute_names(self, key: impl Into<String>, value: impl Into<String>) -> Self;
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self;
}

impl GuardBuilder for aws_sdk_dynamodb::types::builders::PutBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self {
        self.condition_expression(expr)
    }
    fn expression_attribute_names(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.expression_attribute_names(key, value)
    }
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.expression_attribute_values(key, value)
    }
}

impl GuardBuilder for aws_sdk_dynamodb::types::builders::DeleteBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self {
        self.condition_expression(expr)
    }
    fn expression_attribute_names(self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.expression_attribute_names(key, value)
    }
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.expression_attribute_values(key, value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::dao::records::{RatingMovementRecord, UserRecord, UserStatsRecord};

    const NOW: &str = "2026-09-15T10:00:00.000Z";

    /// A `Dao` whose client is never sent anything. Building a request needs
    /// a client but not a connection, credentials or a table, so the tests
    /// below inspect exactly the request a real call would send — which is as
    /// close to the write as `cargo test` gets without DynamoDB.
    fn offline_dao() -> Dao {
        let config = aws_sdk_dynamodb::Config::builder()
            .behavior_version(aws_sdk_dynamodb::config::BehaviorVersion::latest())
            .region(aws_sdk_dynamodb::config::Region::new("eu-west-2"))
            .build();
        Dao::new(aws_sdk_dynamodb::Client::from_conf(config), "agon")
    }

    fn contribution() -> RatingContributionRecord {
        RatingContributionRecord {
            user_id: "u1".into(),
            ladder: "squash".into(),
            side_id: "sideA".into(),
            played_at: "2026-06-01T10:00:00.000Z".into(),
            movement: RatingMovementRecord {
                mu_before: 25.0,
                sigma_before: 25.0 / 3.0,
                mu_after: 27.6,
                sigma_after: 7.1,
                display_delta: 78,
            },
            applied_at: "2026-06-01T11:00:00.000Z".into(),
        }
    }

    fn rating(mu: f64, matches_rated: u64) -> RatingRecord {
        RatingRecord {
            mu,
            sigma: 7.1,
            matches_rated,
            last_rated_at: "2026-06-01T10:00:00.000Z".into(),
        }
    }

    fn profile_key(user_id: &str) -> HashMap<String, AttributeValue> {
        HashMap::from([
            (ATTR_PK.to_string(), s(Pk::User(user_id.into()).to_string())),
            (ATTR_SK.to_string(), s(Sk::Profile.to_string())),
        ])
    }

    /// Every `#name` (or `:value`) placeholder the expressions mention.
    fn placeholders(exprs: &[&str], sigil: char) -> BTreeSet<String> {
        exprs
            .iter()
            .flat_map(|expr| expr.split(sigil).skip(1))
            .map(|rest| {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                format!("{sigil}{name}")
            })
            .collect()
    }

    /// DynamoDB rejects a request whose expressions mention a placeholder it
    /// does not define — and, less obviously, one that defines a placeholder
    /// no expression mentions. Both are a `ValidationException` at send time,
    /// which nothing in `cargo test` otherwise reaches, so every request built
    /// here is held to both directions.
    fn assert_binds_exactly(
        what: &str,
        exprs: &[&str],
        names: Option<&HashMap<String, String>>,
        values: Option<&HashMap<String, AttributeValue>>,
    ) {
        let bound_names: BTreeSet<String> =
            names.into_iter().flat_map(|m| m.keys().cloned()).collect();
        let bound_values: BTreeSet<String> =
            values.into_iter().flat_map(|m| m.keys().cloned()).collect();
        assert_eq!(placeholders(exprs, '#'), bound_names, "{what}: #names");
        assert_eq!(placeholders(exprs, ':'), bound_values, "{what}: :values");
    }

    fn assert_request_binds_exactly(what: &str, req: &UpdateItemFluentBuilder) {
        let exprs: Vec<&str> = [req.get_update_expression(), req.get_condition_expression()]
            .into_iter()
            .filter_map(|e| e.as_deref())
            .collect();
        assert_binds_exactly(
            what,
            &exprs,
            req.get_expression_attribute_names().as_ref(),
            req.get_expression_attribute_values().as_ref(),
        );
    }

    fn assert_transaction_binds_exactly(what: &str, tx: &[TransactWriteItem]) {
        for (i, item) in tx.iter().enumerate() {
            let what = format!("{what}[{i}]");
            if let Some(put) = item.put() {
                let exprs: Vec<&str> = put.condition_expression().into_iter().collect();
                assert_binds_exactly(
                    &what,
                    &exprs,
                    put.expression_attribute_names(),
                    put.expression_attribute_values(),
                );
            } else if let Some(update) = item.update() {
                let exprs: Vec<&str> = std::iter::once(update.update_expression())
                    .chain(update.condition_expression())
                    .collect();
                assert_binds_exactly(
                    &what,
                    &exprs,
                    update.expression_attribute_names(),
                    update.expression_attribute_values(),
                );
            } else if let Some(delete) = item.delete() {
                let exprs: Vec<&str> = delete.condition_expression().into_iter().collect();
                assert_binds_exactly(
                    &what,
                    &exprs,
                    delete.expression_attribute_names(),
                    delete.expression_attribute_values(),
                );
            } else {
                panic!("{what}: an item kind this module never builds");
            }
        }
    }

    /// Consent is first-writer-wins and never conjures a profile, and both
    /// halves live entirely in this request's condition — there is no read
    /// to fall back on. So: the update sets the one ladder's path (a `SET` of
    /// the whole map would erase every other ladder's opt-in); the condition
    /// refuses a ladder that is already set (so a second tick never moves the
    /// original date, and is how the op knows to return `false`) and a
    /// missing profile (an `UpdateItem` on a missing key would create one);
    /// and the ladder travels as a `#name` placeholder rather than being
    /// spliced into the expression.
    #[test]
    fn opting_in_sets_only_an_unset_ladder_on_an_existing_profile() {
        let dao = offline_dao();
        let req = dao.opt_in_request("u1", "squash", NOW);

        assert_eq!(req.get_key().as_ref(), Some(&profile_key("u1")));
        assert_eq!(
            req.get_update_expression().as_deref(),
            Some("SET #opt_ins.#ladder = :opted_in_at")
        );
        assert_eq!(
            req.get_condition_expression().as_deref(),
            Some("attribute_exists(#pk) AND attribute_not_exists(#opt_ins.#ladder)")
        );

        let names = req.get_expression_attribute_names().as_ref().unwrap();
        assert_eq!(names["#pk"], ATTR_PK);
        assert_eq!(names["#opt_ins"], ATTR_RATING_OPT_INS);
        assert_eq!(names["#ladder"], "squash");
        let values = req.get_expression_attribute_values().as_ref().unwrap();
        assert_eq!(values[":opted_in_at"], s(NOW));
    }

    /// The step that makes the nested `SET` resolvable on a profile written
    /// before these maps existed. `if_not_exists` is what stops it wiping a
    /// map that is already there — and with it every opt-in or rating it
    /// holds — and `attribute_exists(#pk)` is the deliberate divergence from
    /// `ensure_stats_sport` that stops it creating a stub profile.
    #[test]
    fn ensuring_a_map_keeps_an_existing_one_and_never_creates_a_profile() {
        let dao = offline_dao();
        for attr in [ATTR_RATINGS, ATTR_RATING_OPT_INS] {
            let req = dao.ensure_profile_map_request("u1", attr);

            assert_eq!(req.get_key().as_ref(), Some(&profile_key("u1")), "{attr}");
            assert_eq!(
                req.get_update_expression().as_deref(),
                Some("SET #map = if_not_exists(#map, :empty)"),
                "{attr}"
            );
            assert_eq!(
                req.get_condition_expression().as_deref(),
                Some("attribute_exists(#pk)"),
                "{attr}"
            );
            let names = req.get_expression_attribute_names().as_ref().unwrap();
            assert_eq!(names["#map"], attr);
            let values = req.get_expression_attribute_values().as_ref().unwrap();
            assert_eq!(
                values[":empty"],
                AttributeValue::M(HashMap::new()),
                "{attr}"
            );
        }
    }

    /// The optimistic lock on `ratings.<ladder>` is the only thing between
    /// two matches on one ladder confirmed at once and a lost update: both
    /// compute from the same base, and without the lock the later write
    /// erases the earlier match's effect. A first rating must find nothing
    /// there; every later one must find exactly what the caller computed
    /// from.
    #[test]
    fn a_rating_write_is_locked_on_the_value_the_caller_read() {
        let dao = offline_dao();
        let before = rating(25.0, 1);
        let after = rating(27.6, 2);

        let first = dao.rating_update("u1", "squash", &after, None).unwrap();
        assert_eq!(first.key(), &profile_key("u1"));
        assert_eq!(first.update_expression(), "SET #ratings.#ladder = :rating");
        assert_eq!(
            first.condition_expression(),
            Some("attribute_exists(#pk) AND attribute_not_exists(#ratings.#ladder)")
        );

        let later = dao
            .rating_update("u1", "squash", &after, Some(&before))
            .unwrap();
        assert_eq!(
            later.condition_expression(),
            Some("attribute_exists(#pk) AND #ratings.#ladder = :expected")
        );
        let values = later.expression_attribute_values().unwrap();
        assert_eq!(
            values[":expected"],
            serde_dynamo::to_attribute_value(&before).unwrap()
        );
        assert_eq!(
            values[":rating"],
            serde_dynamo::to_attribute_value(&after).unwrap()
        );
        assert_eq!(
            later.expression_attribute_names().unwrap()["#ladder"],
            "squash"
        );
    }

    /// The handler decides "nothing changed" with `has_same_effect_as`, and
    /// the write's guard has to agree with it field for field — otherwise a
    /// concurrent change could slip past the guard while the handler
    /// believed the state was the one it read. Above all, neither may look at
    /// `applied_at`: it is a fresh wall clock on every delivery, so guarding
    /// on it would turn every legitimate redelivery into a `Conflict`.
    /// (`records::tests::a_redelivered_contribution_differs_only_in_applied_at`
    /// pins the other half: that each of these fields is one
    /// `has_same_effect_as` compares.)
    #[test]
    fn the_contribution_guard_compares_the_effect_and_not_applied_at() {
        let delete = guard_contribution(
            Delete::builder().table_name("agon").key(ATTR_PK, s("x")),
            &contribution(),
        )
        .unwrap()
        .build()
        .unwrap();

        let guarded: BTreeSet<&str> = delete
            .expression_attribute_names()
            .unwrap()
            .values()
            .map(String::as_str)
            .collect();
        assert_eq!(
            guarded,
            BTreeSet::from(["user_id", "ladder", "side_id", "played_at", "movement"])
        );
        assert!(
            !delete
                .condition_expression()
                .unwrap()
                .contains("applied_at")
        );
    }

    /// A re-applied contribution whose `played_at` or ladder moved writes its
    /// history at a new key, so the old row has to go in the same transaction
    /// — left behind, the chart shows the match twice and a replay counts it
    /// twice. An unchanged re-apply (a redelivery, differing only in
    /// `applied_at`) must not grow a spurious delete.
    #[test]
    fn a_moved_contribution_deletes_its_old_history_entry_in_the_same_transaction() {
        let dao = offline_dao();
        let stored = contribution();
        let r = rating(27.6, 1);

        for moved in [
            RatingContributionRecord {
                played_at: "2026-06-08T10:00:00.000Z".into(),
                ..stored.clone()
            },
            RatingContributionRecord {
                ladder: "tennis".into(),
                ..stored.clone()
            },
        ] {
            let tx = dao
                .rating_contribution_items("m1", &moved, Some(&stored), &r, Some(&r))
                .unwrap();
            let deletes: Vec<&Delete> = tx.iter().filter_map(TransactWriteItem::delete).collect();
            assert_eq!(deletes.len(), 1, "{moved:?}");
            assert_eq!(
                deletes[0].key()[ATTR_PK],
                s(Pk::User("u1".into()).to_string())
            );
            assert_eq!(
                deletes[0].key()[ATTR_SK],
                s(stored.history_sk("m1").to_string())
            );
        }

        let redelivered = RatingContributionRecord {
            applied_at: "2026-06-02T09:30:00.000Z".into(),
            ..stored.clone()
        };
        let tx = dao
            .rating_contribution_items("m1", &redelivered, Some(&stored), &r, Some(&r))
            .unwrap();
        assert_eq!(tx.len(), 3, "contribution, rating, history — no delete");
        assert!(tx.iter().all(|item| item.delete().is_none()));
    }

    /// See [`assert_binds_exactly`]: an unbound or unused placeholder is a
    /// `ValidationException` that only a real send would find. Covers every
    /// request this module builds, in every shape it builds them — first
    /// write and guarded rewrite, moved and unmoved history, withdrawal.
    #[test]
    fn every_request_binds_exactly_the_placeholders_it_uses() {
        let dao = offline_dao();
        let stored = contribution();
        let moved = RatingContributionRecord {
            played_at: "2026-06-08T10:00:00.000Z".into(),
            ..stored.clone()
        };
        let r = rating(27.6, 1);

        assert_request_binds_exactly("opt-in", &dao.opt_in_request("u1", "squash", NOW));
        for attr in [ATTR_RATINGS, ATTR_RATING_OPT_INS] {
            assert_request_binds_exactly(attr, &dao.ensure_profile_map_request("u1", attr));
        }
        assert_transaction_binds_exactly(
            "first apply",
            &dao.rating_contribution_items("m1", &stored, None, &r, None)
                .unwrap(),
        );
        assert_transaction_binds_exactly(
            "re-apply",
            &dao.rating_contribution_items("m1", &stored, Some(&stored), &r, Some(&r))
                .unwrap(),
        );
        assert_transaction_binds_exactly(
            "moved re-apply",
            &dao.rating_contribution_items("m1", &moved, Some(&stored), &r, Some(&r))
                .unwrap(),
        );
        assert_transaction_binds_exactly(
            "withdrawal",
            &dao.withdrawal_items("m1", &stored).unwrap(),
        );
    }

    /// The expressions here name attributes by string — `ratings`,
    /// `rating_opt_ins`, and each field the contribution guard compares —
    /// while the records name them by Rust field. A rename on the record side
    /// compiles cleanly and then fails only in production: writes land in an
    /// attribute nothing reads, or a guard compares an attribute that no
    /// longer exists and every redelivery becomes a permanent `Conflict`.
    #[test]
    fn every_attribute_an_expression_names_exists_on_its_record() {
        let user = UserRecord {
            id: "u1".into(),
            email: "sofia@example.com".into(),
            name: "Sofia".into(),
            profile_image_url: None,
            follower_count: 0,
            following_count: 0,
            unread_count: 0,
            stats: UserStatsRecord::default(),
            ratings: HashMap::from([("squash".to_string(), rating(27.6, 1))]),
            rating_opt_ins: HashMap::from([("squash".to_string(), NOW.to_string())]),
            created_at: NOW.into(),
        };
        let item: HashMap<String, AttributeValue> = serde_dynamo::to_item(&user).unwrap();
        for attr in [ATTR_RATINGS, ATTR_RATING_OPT_INS] {
            assert!(item.contains_key(attr), "UserRecord has no `{attr}`");
        }

        let c = contribution();
        let item: HashMap<String, AttributeValue> = serde_dynamo::to_item(&c).unwrap();
        let delete = guard_contribution(
            Delete::builder().table_name("agon").key(ATTR_PK, s("x")),
            &c,
        )
        .unwrap()
        .build()
        .unwrap();
        for attr in delete.expression_attribute_names().unwrap().values() {
            assert!(
                item.contains_key(attr),
                "RatingContributionRecord has no `{attr}`"
            );
        }
    }

    /// The two records describe the same event from two directions, so every
    /// shared field has to actually be shared. If they could drift, a history
    /// item could be written that the contribution's key never addresses —
    /// and `withdraw_rating_contribution` would then leave it behind forever,
    /// double-counting the match on the next replay.
    #[test]
    fn the_history_entry_mirrors_the_contribution() {
        let c = contribution();
        let entry = c.history_entry("m1");
        assert_eq!(entry.ladder, c.ladder);
        assert_eq!(entry.played_at, c.played_at);
        assert_eq!(entry.movement, c.movement);
        assert_eq!(entry.applied_at, c.applied_at);
        assert_eq!(entry.match_id, "m1");

        assert_eq!(
            c.history_sk("m1"),
            Sk::Rating {
                ladder: "squash".into(),
                played_at: "2026-06-01T10:00:00.000Z".into(),
                match_id: "m1".into(),
            }
        );
    }

    /// A rescheduled or re-sported match lands its history at a *different*
    /// key. This is the comparison `rating_contribution_items` makes to decide
    /// whether to delete the old one; if it ever returned equal for these, the
    /// chart would show the match twice and a replay would count it twice.
    #[test]
    fn moving_a_match_in_time_or_ladder_orphans_its_old_history_key() {
        let before = contribution();

        let rescheduled = RatingContributionRecord {
            played_at: "2026-06-08T10:00:00.000Z".into(),
            ..before.clone()
        };
        assert_ne!(before.history_sk("m1"), rescheduled.history_sk("m1"));

        let resported = RatingContributionRecord {
            ladder: "tennis".into(),
            ..before.clone()
        };
        assert_ne!(before.history_sk("m1"), resported.history_sk("m1"));

        // ...and a redelivery of the unchanged match keeps the same key, so
        // no spurious delete joins the transaction.
        let redelivered = RatingContributionRecord {
            applied_at: "2026-06-02T09:30:00.000Z".into(),
            ..before.clone()
        };
        assert_eq!(before.history_sk("m1"), redelivered.history_sk("m1"));
    }

    /// The history range for a ladder must start at or below the first real
    /// key and end above the last, or a replay silently drops matches at the
    /// edges. `Sk`'s own tests cover the bound arithmetic; this one pins the
    /// two bounds [`Dao::list_rating_history`] actually builds its unbounded
    /// query from.
    #[test]
    fn the_unbounded_history_query_covers_the_whole_ladder() {
        let first = Sk::Rating {
            ladder: "squash".into(),
            played_at: "2020-01-01T00:00:00.000Z".into(),
            match_id: "m0".into(),
        }
        .to_string();
        let last = Sk::Rating {
            ladder: "squash".into(),
            played_at: "2099-12-31T23:59:59.999Z".into(),
            match_id: "mZ".into(),
        }
        .to_string();

        let low = Sk::rating_prefix("squash");
        let high = Sk::rating_history_end("squash");
        assert!(low <= first && first <= high);
        assert!(low <= last && last <= high);
    }
}
