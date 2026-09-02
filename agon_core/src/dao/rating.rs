//! Rating storage: an account's per-ladder rating, its rating history, and
//! what each match contributed to it.
//!
//! Three item shapes, written together and read apart:
//!
//! | What | Where | Why |
//! |---|---|---|
//! | current rating | `ratings.<ladder>` on `USER#<uid>`/`#PROFILE` (or `TEAM#<tid>`/`#META`) | rides the point read profiles, feed and search hydration already do |
//! | history | `USER#<uid>` / `RATING#<ladder>#<played_at>#<mid>` | replay source *and* the rating-over-time chart, from one write |
//! | contribution | `MATCH#<mid>` / `RATINGCONTRIB#<ownerId>` | makes re-rating idempotent, and is what the UI reads to show "+18" |
//!
//! ## Users and teams are the same code path
//!
//! Every operation here takes a [`RatingOwner`] rather than a user id. A team
//! carries ratings of its own (`TeamRecord::ratings`), and — the part that
//! decides it — a team's rating has to be *repairable*, which means replaying
//! it, which means it needs history items exactly like a user's. Writing
//! history for users only would leave team ratings permanently
//! unreconstructable after a re-score, so the two owner kinds differ in
//! nothing but which `Pk`/`Sk` pair addresses their profile item.
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
//! handler that re-rates A against the account's *current* rating computes a
//! different movement (it now starts from B's output), sees it differ from
//! the stored contribution, and applies it — counting A twice.
//!
//! The right test re-rates A from the ratings the participants carried *into*
//! A, which is precisely what [`RatingContributionRecord::movement`]'s
//! `mu_before`/`sigma_before` preserve. So:
//!
//! 1. [`Dao::list_rating_contributions`] for the match.
//! 2. If it is non-empty, rebuild the sides from those `before` ratings and
//!    `side_id`s and re-run `rating::rate_sides` with the match's *current*
//!    `winner_side_id`.
//! 3. Compare with [`RatingContributionRecord::has_same_effect_as`]. Equal
//!    means genuinely nothing changed — do nothing. Different means the match
//!    itself changed (re-score, roster edit, sport edit), which is a repair,
//!    not an incremental apply.
//! 4. Only an *empty* contribution set is a first rating, and only that case
//!    reads the participants' current ratings.
//!
//! That is why the contribution stores each participant's incoming rating and
//! `side_id` at all: the collection for a match is a self-sufficient replay
//! input that survives the roster changing underneath it.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};
use serde::Deserialize;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{
    RatingContributionRecord, RatingHistoryRecord, RatingOwnerKindRecord, RatingRecord,
};

/// Type tag for the per-match rating-contribution item.
pub const TYPE_RATING_CONTRIBUTION: &str = "rating_contribution";
/// Type tag for a rating-history item.
pub const TYPE_RATING_HISTORY: &str = "rating_history";

/// The profile-item attribute holding the per-ladder rating map. Must stay in
/// step with the field name on **both** `UserRecord::ratings` and
/// `TeamRecord::ratings` — a rename there without a change here compiles
/// cleanly and then writes ratings into an attribute nothing reads.
const ATTR_RATINGS: &str = "ratings";

/// Who a rating belongs to: an account or a team.
///
/// Exists so the ops below can address either owner's profile item without
/// caring which it is. The mapping is the whole content of the type, and it
/// is easy to get subtly wrong — a team's record lives under `#META`, not the
/// `#PROFILE` a user's does — so it is pinned by
/// `tests::owner_keys_address_the_right_profile_item`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatingOwner {
    pub kind: RatingOwnerKindRecord,
    pub id: String,
}

impl RatingOwner {
    #[must_use]
    pub fn user(id: impl Into<String>) -> Self {
        Self {
            kind: RatingOwnerKindRecord::User,
            id: id.into(),
        }
    }

    #[must_use]
    pub fn team(id: impl Into<String>) -> Self {
        Self {
            kind: RatingOwnerKindRecord::Team,
            id: id.into(),
        }
    }

    /// The partition the owner's profile item and rating history both live
    /// in.
    #[must_use]
    pub fn pk(&self) -> Pk {
        match self.kind {
            RatingOwnerKindRecord::User => Pk::User(self.id.clone()),
            RatingOwnerKindRecord::Team => Pk::Team(self.id.clone()),
        }
    }

    /// The sort key of the item the `ratings` map is stored on.
    #[must_use]
    pub fn profile_sk(&self) -> Sk {
        match self.kind {
            RatingOwnerKindRecord::User => Sk::Profile,
            RatingOwnerKindRecord::Team => Sk::Meta,
        }
    }

    /// For error messages — `user u_abc` / `team t_xyz`.
    fn describe(&self) -> String {
        match self.kind {
            RatingOwnerKindRecord::User => format!("user {}", self.id),
            RatingOwnerKindRecord::Team => format!("team {}", self.id),
        }
    }
}

impl From<&RatingContributionRecord> for RatingOwner {
    fn from(contribution: &RatingContributionRecord) -> Self {
        Self {
            kind: contribution.owner_kind,
            id: contribution.owner_id.clone(),
        }
    }
}

impl RatingContributionRecord {
    /// The history entry this contribution implies for its owner.
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

    /// The sort key of that history entry, in the owner's partition.
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
/// A projection read rather than deserializing the whole `UserRecord` /
/// `TeamRecord`: not for the RCUs (DynamoDB charges for the whole item
/// either way) but because it is the one shape both owner kinds share, so
/// reading a rating needs no branch on which record type the item holds.
#[derive(Debug, Deserialize)]
struct RatingsProjection {
    #[serde(default)]
    ratings: HashMap<String, RatingRecord>,
}

impl Dao {
    /// Every ladder this owner is rated on.
    ///
    /// An owner that does not exist and one that exists but has never been
    /// rated both come back as an empty map. That conflation is deliberate:
    /// the only caller is about to write, and every write below is guarded on
    /// `attribute_exists(PK)`, so a missing owner is caught there — with a
    /// `NotFound` naming it — rather than here, where distinguishing the two
    /// would cost a second read on every rated match.
    #[tracing::instrument(skip(self))]
    pub async fn get_ratings(
        &self,
        owner: &RatingOwner,
    ) -> DaoResult<HashMap<String, RatingRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(owner.pk().to_string()))
            .key(ATTR_SK, s(owner.profile_sk().to_string()))
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

    /// This owner's rating on one ladder, or `None` if they have never been
    /// rated on it.
    ///
    /// `None` is not the same as "1500 with maximum uncertainty" *to this
    /// layer* even though the engine treats them identically
    /// (`rating::group_by_side` defaults an absent player). The difference
    /// matters one level up: `None` is what makes the first write's
    /// `attribute_not_exists` guard meaningful, so a rating cannot be created
    /// twice by two racing first matches.
    #[tracing::instrument(skip(self))]
    pub async fn get_rating(
        &self,
        owner: &RatingOwner,
        ladder: &str,
    ) -> DaoResult<Option<RatingRecord>> {
        Ok(self.get_ratings(owner).await?.remove(ladder))
    }

    /// Set one ladder's rating, guarded on the value the caller read.
    ///
    /// Standalone counterpart to the three-item write in
    /// [`Self::apply_rating_contribution`] — for the paths that move a rating
    /// without a match behind it (σ-inflation on inactivity, and the tail of
    /// a repair replay, whose per-match writes have already gone through
    /// `apply_rating_contribution`).
    ///
    /// `expected` is the rating read before computing the new one, or `None`
    /// if there was none. Passing the wrong one is a `Conflict`, not a silent
    /// overwrite: two matches on the same ladder confirmed at the same
    /// instant would otherwise both compute from the same base and the loser
    /// would vanish.
    #[tracing::instrument(skip(self, rating, expected))]
    pub async fn put_rating(
        &self,
        owner: &RatingOwner,
        ladder: &str,
        rating: &RatingRecord,
        expected: Option<&RatingRecord>,
    ) -> DaoResult<()> {
        self.ensure_ratings_map(owner).await?;
        let update = self.rating_update(owner, ladder, rating, expected)?;
        self.run_guarded(
            vec![TransactWriteItem::builder().update(update).build()],
            || {
                DaoError::Conflict(format!(
                    "rating for {} on ladder {ladder} changed concurrently",
                    owner.describe()
                ))
            },
        )
        .await
    }

    /// Apply one participant's rating movement from one match: the new
    /// rating, the contribution that produced it and the history entry
    /// recording it, as a single transaction.
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
    /// participant's — see the module doc, which spells out the
    /// double-counting bug that the obvious per-participant version walks
    /// into.
    ///
    /// `stored` / `stored_rating` are the values the caller read before
    /// computing, and become the optimistic-lock guards. `Ok(())` means the
    /// state the caller computed against was still current when the write
    /// landed.
    #[tracing::instrument(skip(self, contribution, stored, rating, stored_rating))]
    pub async fn apply_rating_contribution(
        &self,
        match_id: &str,
        contribution: &RatingContributionRecord,
        stored: Option<&RatingContributionRecord>,
        rating: &RatingRecord,
        stored_rating: Option<&RatingRecord>,
    ) -> DaoResult<()> {
        let owner = RatingOwner::from(contribution);
        self.ensure_ratings_map(&owner).await?;

        let contrib_pk = Pk::Match(match_id.into()).to_string();
        let contrib_sk = Sk::RatingContribution(contribution.owner_id.clone()).to_string();

        let mut tx: Vec<TransactWriteItem> = Vec::new();

        // 1. The contribution itself, guarded on what we read.
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::RatingContribution(contribution.owner_id.clone()),
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

        // 2. The owner's rating on this ladder.
        tx.push(
            TransactWriteItem::builder()
                .update(self.rating_update(&owner, &contribution.ladder, rating, stored_rating)?)
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
                            &owner.pk(),
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
        //    rescheduled, the history entry we just wrote is at a *different*
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
                                .key(ATTR_PK, s(owner.pk().to_string()))
                                .key(ATTR_SK, s(previous_sk.to_string()))
                                .build()
                                .map_err(|e| DaoError::Dynamo(e.to_string()))?,
                        )
                        .build(),
                );
            }
        }

        self.run_guarded(tx, || {
            DaoError::Conflict(format!(
                "rating state for match {match_id} / {contrib_pk} {contrib_sk} changed concurrently"
            ))
        })
        .await
    }

    /// Back a match's contribution out of one participant: delete the
    /// contribution item and its history entry, in one transaction.
    ///
    /// For a match that stopped being rateable — cancelled, flipped to
    /// friendly, or a player dropped from the roster.
    ///
    /// It deliberately does **not** touch the stored rating, which is the
    /// obvious thing to expect it to do and is not possible: a Weng-Lin
    /// update is not invertible, so "subtract this match" has no closed form
    /// — the only way back is to replay the ladder's history from this match
    /// forward, which is `RepairRatings`' job. Removing the two items first
    /// is what makes that replay produce the right answer, so this is the
    /// step before the repair, not a substitute for it. Until the repair
    /// runs, the owner's `ratings.<ladder>` still includes this match's
    /// effect; the history no longer explains it.
    #[tracing::instrument(skip(self, stored))]
    pub async fn withdraw_rating_contribution(
        &self,
        match_id: &str,
        stored: &RatingContributionRecord,
    ) -> DaoResult<()> {
        let owner = RatingOwner::from(stored);

        let delete = guard_contribution(
            Delete::builder()
                .table_name(self.table())
                .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
                .key(
                    ATTR_SK,
                    s(Sk::RatingContribution(stored.owner_id.clone()).to_string()),
                ),
            stored,
        )?
        .build()
        .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // Unguarded, and it has to be: the history row is a projection of the
        // contribution row, so guarding both would fail the whole transaction
        // on a redelivery that had already removed them — which is exactly
        // the case that must succeed quietly. The guarded contribution delete
        // above is what serialises concurrent withdrawals.
        let delete_history = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(owner.pk().to_string()))
            .key(ATTR_SK, s(stored.history_sk(match_id).to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        self.run_guarded(
            vec![
                TransactWriteItem::builder().delete(delete).build(),
                TransactWriteItem::builder().delete(delete_history).build(),
            ],
            || {
                DaoError::Conflict(format!(
                    "rating contribution for match {match_id} changed concurrently"
                ))
            },
        )
        .await
    }

    /// This match's rating contribution for one participant, if it has been
    /// rated for them.
    #[tracing::instrument(skip(self))]
    pub async fn get_rating_contribution(
        &self,
        match_id: &str,
        owner_id: &str,
    ) -> DaoResult<Option<RatingContributionRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(
                ATTR_SK,
                s(Sk::RatingContribution(owner_id.into()).to_string()),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        out.item.map(from_item).transpose()
    }

    /// Every rating contribution stored for a match.
    ///
    /// Returns whole records where the stats analogue
    /// (`list_stat_contribution_user_ids`) returns bare ids, for two reasons:
    /// an id alone doesn't say whether it names a user or a team (that lives
    /// in `owner_kind`, not the key — see `Sk::RatingContribution`), and the
    /// records *are* the replay input the change-detection protocol in this
    /// module's header runs on. Fetching ids and then re-reading each record
    /// would be a round trip per participant for data the query already
    /// carried.
    ///
    /// Unpaginated, like its stats counterpart: this is one item per
    /// participant of one match, which is bounded by the roster.
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

    /// One ladder's rating history for an owner, **oldest first**, from
    /// `from` (a `played_at`, inclusive) onwards if given.
    ///
    /// Ascending, unlike `list_feed`'s newest-first, and there is no
    /// direction flag: both readers of this collection want played order.
    /// Repair replays forwards by definition, and the rating-over-time chart
    /// plots left to right. A newest-first mode would be a second sort
    /// direction to keep correct for the sake of a reversal the caller can do
    /// on a page it already holds.
    ///
    /// Paginated because it is the replay source. A three-year-old account's
    /// squash history is thousands of items, and `RepairRatings` is a
    /// checkpointed chunked workflow precisely so it never has to hold all of
    /// them — the cursor is what it checkpoints.
    #[tracing::instrument(skip(self))]
    pub async fn list_rating_history(
        &self,
        owner: &RatingOwner,
        ladder: &str,
        from: Option<&str>,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<RatingHistoryRecord>> {
        // `BETWEEN` rather than `begins_with`, because DynamoDB allows only
        // one sort-key condition and the resumable form needs a lower bound
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
                .expression_attribute_values(":pk", s(owner.pk().to_string()))
                .expression_attribute_values(":lo", s(low))
                .expression_attribute_values(":hi", s(Sk::rating_history_end(ladder)))
                .scan_index_forward(true),
            cursor,
            limit,
        )
        .await
    }

    /// The `SET ratings.<ladder> = :rating` update, guarded on `expected` and
    /// on the owner still existing.
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
        owner: &RatingOwner,
        ladder: &str,
        rating: &RatingRecord,
        expected: Option<&RatingRecord>,
    ) -> DaoResult<Update> {
        let mut b = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(owner.pk().to_string()))
            .key(ATTR_SK, s(owner.profile_sk().to_string()))
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

    /// Make sure the owner's profile item has a `ratings` map, so the nested
    /// `SET ratings.#ladder = ...` above has somewhere to resolve into.
    ///
    /// A separate round trip rather than a clause in the same expression, for
    /// the same reason `ensure_stats_sport` is: `SET ratings =
    /// if_not_exists(ratings, :empty)` and `SET ratings.#ladder = :r`
    /// overlap on one document path, which DynamoDB rejects outright — and
    /// splitting them across two items of one transaction is not possible
    /// either, since a transaction may not touch the same item twice. It only
    /// runs on a real rating write (a confirmed ranked match), which is rare
    /// per account.
    ///
    /// Unlike `ensure_stats_sport` this is conditional on the item existing.
    /// Without that, a rating write naming a deleted account would *create* a
    /// stub item holding nothing but `PK`/`SK`/`ratings` — a profile-shaped
    /// row that every read would then fail to deserialize.
    async fn ensure_ratings_map(&self, owner: &RatingOwner) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(owner.pk().to_string()))
            .key(ATTR_SK, s(owner.profile_sk().to_string()))
            .update_expression("SET #ratings = if_not_exists(#ratings, :empty)")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#ratings", ATTR_RATINGS)
            .expression_attribute_values(":empty", AttributeValue::M(HashMap::new()))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(owner.describe()))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Run a transaction, mapping a failed condition guard to a `Conflict`
    /// carrying `on_conflict`'s message.
    ///
    /// A `Conflict` here is not an error the caller should swallow: it means
    /// another writer moved the state between the caller's read and this
    /// write, so the message must be retried and recomputed from fresh state.
    /// Every rating write is driven by an at-least-once stream event, so
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
/// legitimate redelivery.
fn guard_contribution<B: GuardBuilder>(b: B, previous: &RatingContributionRecord) -> DaoResult<B> {
    // `#name` placeholders throughout rather than the bare attribute names
    // `stats::guard` uses. These names (`ladder`, `movement`, …) are ours to
    // choose and DynamoDB's reserved-word list is long enough that checking
    // it by eye on every new field is a worse bet than always escaping.
    Ok(b.condition_expression(
        "#owner_kind = :kind AND #owner_id = :owner AND #ladder = :ladder \
         AND #side_id = :side AND #played_at = :played AND #movement = :movement",
    )
    .expression_attribute_names("#owner_kind", "owner_kind")
    .expression_attribute_names("#owner_id", "owner_id")
    .expression_attribute_names("#ladder", "ladder")
    .expression_attribute_names("#side_id", "side_id")
    .expression_attribute_names("#played_at", "played_at")
    .expression_attribute_names("#movement", "movement")
    .expression_attribute_values(
        ":kind",
        serde_dynamo::to_attribute_value(previous.owner_kind)?,
    )
    .expression_attribute_values(":owner", s(&previous.owner_id))
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
/// different records and share nothing but the two method names, and hoisting
/// it would make `stats` and `rating` co-vary for no reason.
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
    use super::*;
    use crate::dao::records::RatingMovementRecord;

    fn contribution() -> RatingContributionRecord {
        RatingContributionRecord {
            owner_kind: RatingOwnerKindRecord::User,
            owner_id: "u1".into(),
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

    /// A team's profile-shaped item is `TEAM#<id>` / `#META`, a user's is
    /// `USER#<id>` / `#PROFILE`. Every rating write addresses one of them by
    /// owner kind alone, so getting this pair wrong would write a team's
    /// rating into a `#PROFILE` item that does not exist — which the
    /// `attribute_exists(PK)` guard turns into a permanent `NotFound` for
    /// every team, on every rated match.
    #[test]
    fn owner_keys_address_the_right_profile_item() {
        let user = RatingOwner::user("u1");
        assert_eq!(user.pk(), Pk::User("u1".into()));
        assert_eq!(user.profile_sk(), Sk::Profile);

        let team = RatingOwner::team("t1");
        assert_eq!(team.pk(), Pk::Team("t1".into()));
        assert_eq!(team.profile_sk(), Sk::Meta);
    }

    /// The owner a contribution is written back to comes from the record, not
    /// from the key — `RATINGCONTRIB#<id>` deliberately doesn't say which
    /// kind of id it holds (see `Sk::RatingContribution`), so this conversion
    /// is the only thing that decides whether a movement lands on a user or a
    /// team.
    #[test]
    fn the_owner_comes_from_the_record_not_the_key() {
        let mut team_contribution = contribution();
        team_contribution.owner_kind = RatingOwnerKindRecord::Team;
        team_contribution.owner_id = "t1".into();
        assert_eq!(
            RatingOwner::from(&team_contribution),
            RatingOwner::team("t1")
        );
        assert_eq!(RatingOwner::from(&contribution()), RatingOwner::user("u1"));
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
    /// key, so the old row has to be deleted in the same transaction that
    /// writes the new one. This is the comparison
    /// `apply_rating_contribution` makes to decide that; if it ever returned
    /// equal for these, the chart would show the match twice and a replay
    /// would count it twice.
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
    /// two bounds this module actually builds a query from.
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
