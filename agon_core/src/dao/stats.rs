//! Per-user, per-sport stats (stored inline on the user's profile item) and
//! reconciling them from a match's current state.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, from_item, item_sk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::StatContributionRecord;

/// Type tag for the per-match stat-contribution item.
pub const TYPE_STAT_CONTRIBUTION: &str = "stat_contribution";

/// A single participant's result in a match, for stats purposes. Sport-
/// agnostic: every sport derives this the same way, from `side_id` vs the
/// match's `winner_side_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    Won,
    Drawn,
    Lost,
}

/// What a match currently contributes to one participant's stats: whether —
/// and how — they played, plus any sport-specific counters (e.g. cricket
/// runs/wickets, football goals/assists) they racked up in it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MatchContribution {
    /// `None` if the user didn't play this match (or it doesn't currently
    /// count — not completed, no confirmed score, dropped from the roster).
    pub outcome: Option<MatchOutcome>,
    /// Sport-specific counters earned in this match. Only meaningful (and
    /// only ever populated by callers) when `outcome` is `Some`.
    pub counters: HashMap<String, u64>,
}

impl StatContributionRecord {
    /// This contribution's fields flattened to a `{name: value}` map — the
    /// typed played/won/drawn/lost counters alongside whatever sport-specific
    /// counters it carries — so the reconciler can diff and apply an
    /// arbitrary set of named counters uniformly instead of hand-listing each
    /// one.
    fn as_counter_map(&self) -> HashMap<String, u64> {
        let mut m = self.counters.clone();
        m.insert("matches_played".to_string(), self.played);
        m.insert("wins".to_string(), self.won);
        m.insert("draws".to_string(), self.drawn);
        m.insert("losses".to_string(), self.lost);
        m
    }
}

impl Dao {
    /// User ids that currently have a stored stat contribution for this match.
    /// The reconciler unions these with the match's current participants so a
    /// player removed from the roster still gets their contribution backed out.
    #[tracing::instrument(skip(self))]
    pub async fn list_stat_contribution_user_ids(&self, match_id: &str) -> DaoResult<Vec<String>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::stat_contribution_prefix()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut ids = Vec::new();
        for item in out.items.unwrap_or_default() {
            if let Sk::StatContribution(uid) = item_sk(&item)? {
                ids.push(uid);
            }
        }
        Ok(ids)
    }

    /// Reconcile one participant's stats with a match's **current** state.
    ///
    /// `sport`/`desired` describe what the match *should* contribute right
    /// now. This is diffed against the contribution we last stored for
    /// `(match, user)` and only the delta is applied to the user's
    /// `stats.<sport>` counters on their profile item — so the same call is:
    ///
    /// - **idempotent**: unchanged state → zero delta → no write (safe under
    ///   at-least-once delivery and the match-meta events fired by every
    ///   like/comment);
    /// - **self-correcting**: a re-score moves `wins`/`draws`/`losses` and any
    ///   sport-specific counters, a late roster add counts the new player, and
    ///   cancelling a completed match (`outcome=None`) backs the counts out —
    ///   including moving counts between sports if the match's sport changed.
    ///
    /// Concurrency: the contribution write is conditional on the value we read,
    /// so two racing reconciles can't both apply a delta — the loser's
    /// transaction fails (`Conflict`) and redelivery re-reads and converges.
    #[tracing::instrument(skip(self, desired))]
    pub async fn reconcile_match_contribution(
        &self,
        match_id: &str,
        user_id: &str,
        sport: &str,
        desired: &MatchContribution,
    ) -> DaoResult<()> {
        let contrib_pk = Pk::Match(match_id.into()).to_string();
        let contrib_sk = Sk::StatContribution(user_id.into()).to_string();

        // What's currently stored for this (match, user), if anything.
        let stored: Option<StatContributionRecord> = {
            let out = self
                .client
                .get_item()
                .table_name(self.table())
                .key(ATTR_PK, s(&contrib_pk))
                .key(ATTR_SK, s(&contrib_sk))
                .send()
                .await
                .map_err(|e| DaoError::Dynamo(e.to_string()))?;
            out.item.map(from_item).transpose()?
        };

        let desired_record = StatContributionRecord {
            match_type: sport.to_string(),
            played: desired.outcome.is_some() as u64,
            won: matches!(desired.outcome, Some(MatchOutcome::Won)) as u64,
            drawn: matches!(desired.outcome, Some(MatchOutcome::Drawn)) as u64,
            lost: matches!(desired.outcome, Some(MatchOutcome::Lost)) as u64,
            counters: desired.counters.clone(),
        };

        // A missing contribution is effectively zero in the desired sport, so a
        // no-op event (e.g. a like on a scheduled match) matches and writes
        // nothing. `old_counters` defaults to empty (every key implicitly 0)
        // rather than being absent, so it compares equal to an explicit
        // all-zero `new_counters` map below instead of spuriously looking
        // "changed" just because no counter was ever stored.
        let old_sport = stored
            .as_ref()
            .map(|c| c.match_type.clone())
            .unwrap_or_else(|| sport.to_string());
        let old_counters = stored
            .as_ref()
            .map(StatContributionRecord::as_counter_map)
            .unwrap_or_default();
        let new_counters = desired_record.as_counter_map();
        let same_sport_deltas = counter_deltas(&old_counters, &new_counters);

        if old_sport == sport && same_sport_deltas.values().all(|d| *d == 0) {
            return Ok(());
        }

        let mut tx: Vec<TransactWriteItem> = Vec::new();

        // 1. Update the contribution item to the desired value (or delete it when
        //    the match no longer contributes), guarded on the value we read.
        if desired_record.played == 0 {
            let mut b = Delete::builder()
                .table_name(self.table())
                .key(ATTR_PK, s(&contrib_pk))
                .key(ATTR_SK, s(&contrib_sk));
            b = match &stored {
                None => b
                    .condition_expression("attribute_not_exists(#pk)")
                    .expression_attribute_names("#pk", ATTR_PK),
                Some(c) => guard(b, c),
            };
            tx.push(
                TransactWriteItem::builder()
                    .delete(b.build().map_err(|e| DaoError::Dynamo(e.to_string()))?)
                    .build(),
            );
        } else {
            let item = to_item(
                &Pk::Match(match_id.into()),
                &Sk::StatContribution(user_id.into()),
                TYPE_STAT_CONTRIBUTION,
                &desired_record,
            )?;
            let mut b = Put::builder().table_name(self.table()).set_item(Some(item));
            b = match &stored {
                None => b
                    .condition_expression("attribute_not_exists(#pk)")
                    .expression_attribute_names("#pk", ATTR_PK),
                Some(c) => guard(b, c),
            };
            tx.push(
                TransactWriteItem::builder()
                    .put(b.build().map_err(|e| DaoError::Dynamo(e.to_string()))?)
                    .build(),
            );
        }

        // 2. Apply the counter delta(s). If the sport changed, back the old
        //    contribution out of the old sport (a no-op if there was none —
        //    `old_counters` is empty) and add the new one; otherwise apply the
        //    net per-counter delta already computed above on the single sport.
        if old_sport != sport {
            let backout: HashMap<String, i64> = old_counters
                .iter()
                .map(|(k, v)| (k.clone(), -(*v as i64)))
                .collect();
            if let Some(u) = self.stats_delta(user_id, &old_sport, &backout).await? {
                tx.push(TransactWriteItem::builder().update(u).build());
            }
            let apply: HashMap<String, i64> = new_counters
                .iter()
                .map(|(k, v)| (k.clone(), *v as i64))
                .collect();
            if let Some(u) = self.stats_delta(user_id, sport, &apply).await? {
                tx.push(TransactWriteItem::builder().update(u).build());
            }
        } else if let Some(u) = self.stats_delta(user_id, sport, &same_sport_deltas).await? {
            tx.push(TransactWriteItem::builder().update(u).build());
        }

        match self
            .client
            .transact_write_items()
            .set_transact_items(Some(tx))
            .send()
            .await
        {
            Ok(_) => {
                // Best-effort personal-best update — deliberately outside the
                // transaction above (see `update_best_figures`), and only
                // when the user actually played: a contribution being
                // withdrawn (outcome -> None) can't set a new best.
                if desired.outcome.is_some() {
                    self.update_best_figures(user_id, sport, match_id, &desired.counters)
                        .await?;
                }
                Ok(())
            }
            // Lost an optimistic-lock race: another reconcile moved the stored
            // contribution between our read and write. Surface it so the message
            // is retried and re-reads the fresh state.
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                format!("stat contribution for match {match_id} changed concurrently"),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Ratchet up `stats.<sport>.best.<counter>` for every counter in
    /// `counters` whose value in *this* match exceeds any previously
    /// recorded best — a personal-best single-match figure ("most wickets in
    /// a game", "most goals in a game") alongside the match it happened in.
    /// Counters at `0` are skipped (nothing to record).
    ///
    /// Deliberately one-directional: if the match that set a record is later
    /// rescored downward, cancelled, or the player is removed from its
    /// roster, the recorded best is NOT revisited — reproducing the true max
    /// would mean scanning every match the user has ever played, which no
    /// existing access pattern supports. An *upward* re-score IS picked up
    /// correctly, since this recomputes fresh from the match's current
    /// contribution on every reconcile.
    ///
    /// Independent of the guarded transaction in
    /// `reconcile_match_contribution`: each counter's update is conditioned
    /// on its own current best, so bundling it into that all-or-nothing
    /// transaction would let one counter with "no improvement here" (an
    /// expected, common case) cancel the legitimate counter-delta update
    /// alongside it. A `ConditionalCheckFailedException` here just means
    /// this match didn't beat the record — not an error.
    async fn update_best_figures(
        &self,
        user_id: &str,
        sport: &str,
        match_id: &str,
        counters: &HashMap<String, u64>,
    ) -> DaoResult<()> {
        let candidates: Vec<(&String, &u64)> = counters.iter().filter(|(_, v)| **v > 0).collect();
        if candidates.is_empty() {
            return Ok(());
        }
        let pk = s(Pk::User(user_id.into()).to_string());
        let sk = s(Sk::Profile.to_string());

        // `stats.#sport` is guaranteed to already exist by this point (the
        // transaction just above always touches at least `matches_played`
        // for a played contribution), but `stats.#sport.best` might not —
        // ensure it's a map before setting counters within it, same
        // two-step reasoning as `ensure_stats_sport`.
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, pk.clone())
            .key(ATTR_SK, sk.clone())
            .update_expression("SET stats.#sport.best = if_not_exists(stats.#sport.best, :empty)")
            .expression_attribute_names("#sport", sport)
            .expression_attribute_values(":empty", AttributeValue::M(HashMap::new()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        for (counter, value) in candidates {
            let new_best = AttributeValue::M(HashMap::from([
                ("value".to_string(), AttributeValue::N(value.to_string())),
                ("match_id".to_string(), s(match_id)),
            ]));
            let result = self
                .client
                .update_item()
                .table_name(self.table())
                .key(ATTR_PK, pk.clone())
                .key(ATTR_SK, sk.clone())
                .update_expression("SET stats.#sport.best.#counter = :new")
                .condition_expression(
                    "attribute_not_exists(stats.#sport.best.#counter) OR stats.#sport.best.#counter.#val < :val",
                )
                .expression_attribute_names("#sport", sport)
                .expression_attribute_names("#counter", counter.as_str())
                .expression_attribute_names("#val", "value")
                .expression_attribute_values(":new", new_best)
                .expression_attribute_values(":val", AttributeValue::N(value.to_string()))
                .send()
                .await;
            match result {
                Ok(_) => {}
                Err(e) if is_update_conditional_failure(&e) => {}
                Err(e) => return Err(DaoError::Dynamo(e.to_string())),
            }
        }
        Ok(())
    }

    /// One `ADD stats.#sport.#counter :d, ...` update on the user's profile
    /// item — one clause per nonzero entry in `deltas` — or `None` if every
    /// delta is zero. `stats.<sport>` and every counter named in `deltas` must
    /// already exist for a nested `ADD` to resolve, so this first ensures they
    /// do (a separate, unconditionally idempotent call — `TransactWriteItems`
    /// can't touch the profile item twice in the one transaction below it
    /// joins). That extra round trip only happens on a real stats change
    /// (a completed match, a re-score, ...), which is rare per user.
    async fn stats_delta(
        &self,
        user_id: &str,
        sport: &str,
        deltas: &HashMap<String, i64>,
    ) -> DaoResult<Option<Update>> {
        let nonzero: Vec<(&String, &i64)> = deltas.iter().filter(|(_, d)| **d != 0).collect();
        if nonzero.is_empty() {
            return Ok(None);
        }
        self.ensure_stats_sport(user_id, sport, nonzero.iter().map(|(k, _)| k.as_str()))
            .await?;

        let mut update_clauses = Vec::with_capacity(nonzero.len());
        let mut b = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Profile.to_string()))
            .expression_attribute_names("#sport", sport);
        for (i, (counter, delta)) in nonzero.iter().enumerate() {
            let name_ph = format!("#c{i}");
            let val_ph = format!(":d{i}");
            update_clauses.push(format!("stats.#sport.{name_ph} {val_ph}"));
            b = b
                .expression_attribute_names(name_ph, counter.as_str())
                .expression_attribute_values(val_ph, AttributeValue::N(delta.to_string()));
        }
        let u = b
            .update_expression(format!("ADD {}", update_clauses.join(", ")))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(Some(u))
    }

    /// Make sure `stats.<sport>` exists as a map, and that each counter named
    /// in `counters` exists within it as `0`, so the nested `ADD` clauses in
    /// [`Self::stats_delta`] always have a numeric attribute to resolve into.
    /// Two sequential updates rather than one: `SET stats.#sport.#c =
    /// if_not_exists(...)` needs `stats.#sport` to already exist as a map in
    /// the *pre-update* item, which a same-expression `SET stats.#sport =
    /// if_not_exists(...)` earlier in the same clause list doesn't guarantee —
    /// DynamoDB evaluates every clause in an expression against the original
    /// item, not against each other's effects.
    async fn ensure_stats_sport<'a>(
        &self,
        user_id: &str,
        sport: &str,
        counters: impl Iterator<Item = &'a str>,
    ) -> DaoResult<()> {
        let pk = s(Pk::User(user_id.into()).to_string());
        let sk = s(Sk::Profile.to_string());

        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, pk.clone())
            .key(ATTR_SK, sk.clone())
            .update_expression("SET stats.#sport = if_not_exists(stats.#sport, :empty)")
            .expression_attribute_names("#sport", sport)
            .expression_attribute_values(":empty", AttributeValue::M(HashMap::new()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut set_clauses = Vec::new();
        let mut b = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, pk)
            .key(ATTR_SK, sk)
            .expression_attribute_names("#sport", sport)
            .expression_attribute_values(":zero", AttributeValue::N("0".to_string()));
        for (i, counter) in counters.enumerate() {
            let name_ph = format!("#c{i}");
            set_clauses.push(format!(
                "stats.#sport.{name_ph} = if_not_exists(stats.#sport.{name_ph}, :zero)"
            ));
            b = b.expression_attribute_names(name_ph, counter);
        }
        if set_clauses.is_empty() {
            return Ok(());
        }
        b.update_expression(format!("SET {}", set_clauses.join(", ")))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}

/// True if a plain (non-transactional) `UpdateItem` failed its
/// `ConditionExpression` — the expected outcome of a losing "is this a new
/// best?" check in [`Dao::update_best_figures`], not a real error.
fn is_update_conditional_failure(
    err: &aws_sdk_dynamodb::error::SdkError<
        aws_sdk_dynamodb::operation::update_item::UpdateItemError,
    >,
) -> bool {
    use aws_sdk_dynamodb::error::SdkError;
    use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
    matches!(
        err,
        SdkError::ServiceError(se) if matches!(se.err(), UpdateItemError::ConditionalCheckFailedException(_))
    )
}

/// Per-key delta (`new - old`) over the union of both maps' keys, treating a
/// key absent from either side as `0` — so a counter that's never been
/// touched compares equal to one explicitly stored as `0`.
fn counter_deltas(old: &HashMap<String, u64>, new: &HashMap<String, u64>) -> HashMap<String, i64> {
    old.keys()
        .chain(new.keys())
        .map(|k| {
            let o = *old.get(k).unwrap_or(&0) as i64;
            let n = *new.get(k).unwrap_or(&0) as i64;
            (k.clone(), n - o)
        })
        .collect()
}

/// Constrain a contribution `Put`/`Delete` to the value we read (optimistic
/// lock) — every field, including the sport-specific `counters` map compared
/// as a whole (DynamoDB supports equality on map-typed attributes directly).
fn guard<B: GuardBuilder>(b: B, c: &StatContributionRecord) -> B {
    let counters = AttributeValue::M(
        c.counters
            .iter()
            .map(|(k, v)| (k.clone(), AttributeValue::N(v.to_string())))
            .collect(),
    );
    b.condition_expression(
        "played = :op AND won = :ow AND drawn = :od AND lost = :ol AND match_type = :omt AND counters = :oc",
    )
    .expression_attribute_values(":op", AttributeValue::N(c.played.to_string()))
    .expression_attribute_values(":ow", AttributeValue::N(c.won.to_string()))
    .expression_attribute_values(":od", AttributeValue::N(c.drawn.to_string()))
    .expression_attribute_values(":ol", AttributeValue::N(c.lost.to_string()))
    .expression_attribute_values(":omt", s(&c.match_type))
    .expression_attribute_values(":oc", counters)
}

/// The subset of `Put`/`Delete` builder methods `guard` needs — lets one
/// function guard either builder instead of duplicating it per-variant.
trait GuardBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self;
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self;
}

impl GuardBuilder for aws_sdk_dynamodb::types::builders::PutBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self {
        self.condition_expression(expr)
    }
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.expression_attribute_values(key, value)
    }
}

impl GuardBuilder for aws_sdk_dynamodb::types::builders::DeleteBuilder {
    fn condition_expression(self, expr: impl Into<String>) -> Self {
        self.condition_expression(expr)
    }
    fn expression_attribute_values(self, key: impl Into<String>, value: AttributeValue) -> Self {
        self.expression_attribute_values(key, value)
    }
}
