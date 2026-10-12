//! Match series operations: create, get, the two guarded updates the
//! `materialize_series` workflow drives generation forward with
//! (`bump_series_occurrences_generated`, `update_series_status`), and the
//! occurrence claim/list operations that make generation idempotent and
//! queryable. Actually generating a series' occurrences into independent
//! matches is not part of this module — that's `agon_worker`'s
//! `materialize_series` workflow, via the ordinary `Dao::create_match` every
//! other match creation path uses.
//!
//! Idempotency note: generation used to derive each occurrence's match id
//! from `(series_id, index)` via a hash, so a retried activity would land on
//! the same id and hit `Conflict` instead of duplicating. That's gone —
//! occurrence match ids are now ordinary random ids (`agon_core::id::new_id`),
//! and idempotency instead comes from [`Dao::claim_series_occurrence`]: a
//! conditional-put "claim" item, keyed on the occurrence's `starts_at` rather
//! than its index (so two occurrences a date-math bug computed onto the same
//! instant collide on this key instead of silently both generating, which an
//! index-keyed scheme couldn't catch), written *before* the match itself. A
//! retry that loses the claim's conditional put reads back whichever match id
//! the winning attempt offered and proceeds with that one instead of minting
//! a second. This item collection is also now the source for
//! [`Dao::list_series_occurrences`] — "what has this series generated, and
//! when" — which the hash scheme gave no way to answer short of scanning for
//! matches by `series_id`.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{LocationRecord, SeriesOccurrenceRecord, SeriesRecord};

pub const TYPE_SERIES: &str = "series";
pub const TYPE_SERIES_OCCURRENCE: &str = "series_occurrence";

impl Dao {
    /// Create a series meta item. `Conflict` if the series id already exists.
    #[tracing::instrument(skip(self, series), fields(series_id = %series.id))]
    pub async fn create_series(&self, series: &SeriesRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Series(series.id.clone()),
            &Sk::Meta,
            TYPE_SERIES,
            series,
        )?;

        let result = self
            .client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(format!(
                "series {} already exists",
                series.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a series by id. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_series(&self, series_id: &str) -> DaoResult<Option<SeriesRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Claim one series occurrence's generation slot: a conditional put
    /// (`attribute_not_exists`) of `occurrence` at
    /// `SERIES#<seriesId>` / `OCCURRENCE#<startsAt>`.
    ///
    /// Returns the record now actually stored at that slot — `occurrence`
    /// itself if this call's put won the race, or whatever an earlier attempt
    /// already wrote if it didn't. The caller must always use the *returned*
    /// `match_id`, never the one it passed in: that substitution is the whole
    /// mechanism that keeps a retried `materialize_series` activity from
    /// generating one occurrence as two different matches. `occurrence`'s own
    /// `match_id` should be a fresh id the caller is only *offering* — nothing
    /// durable should reference it anywhere until this call confirms it won.
    ///
    /// Idempotent by construction rather than by any special-casing here: a
    /// retry passes the same `(series_id, starts_at)` and a *different* fresh
    /// `match_id` offer (it has no way to know the first attempt's), and still
    /// converges, because every caller reads the claim back out of the
    /// returned record rather than trusting what it sent in.
    #[tracing::instrument(skip(self, occurrence), fields(series_id = %occurrence.series_id, starts_at = %occurrence.starts_at))]
    pub async fn claim_series_occurrence(
        &self,
        occurrence: &SeriesOccurrenceRecord,
    ) -> DaoResult<SeriesOccurrenceRecord> {
        let item = to_item(
            &Pk::Series(occurrence.series_id.clone()),
            &Sk::Occurrence(occurrence.starts_at.clone()),
            TYPE_SERIES_OCCURRENCE,
            occurrence,
        )?;

        let result = self
            .client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(occurrence.clone()),
            Err(e) if is_put_conditional_failure(&e) => {
                let out = self
                    .client
                    .get_item()
                    .table_name(self.table())
                    .key(
                        ATTR_PK,
                        s(Pk::Series(occurrence.series_id.clone()).to_string()),
                    )
                    .key(
                        ATTR_SK,
                        s(Sk::Occurrence(occurrence.starts_at.clone()).to_string()),
                    )
                    .send()
                    .await
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                match out.item {
                    Some(item) => Ok(from_item(item)?),
                    None => Err(DaoError::Dynamo(format!(
                        "series {} occurrence claim at {} vanished after a conditional-put conflict",
                        occurrence.series_id, occurrence.starts_at
                    ))),
                }
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List a series' claimed occurrences, ordered by `starts_at` (the SK) —
    /// "what has this series generated, and when." Includes the first
    /// occurrence (claimed synchronously at series-creation time) alongside
    /// every one `materialize_series` has generated so far.
    #[tracing::instrument(skip(self))]
    pub async fn list_series_occurrences(
        &self,
        series_id: &str,
    ) -> DaoResult<Vec<SeriesOccurrenceRecord>> {
        self.query_series_collection(series_id, &Sk::occurrence_prefix())
            .await
    }

    /// Read every item in a series' partition whose SK starts with
    /// `sk_prefix` (currently only `OCCURRENCE#`), draining all query pages.
    /// Mirrors `match_ops::query_match_collection`, scoped to `Pk::Series`
    /// instead — kept as its own copy rather than generalized over `Pk`,
    /// since nothing else needs that generality yet.
    async fn query_series_collection<T: serde::de::DeserializeOwned>(
        &self,
        series_id: &str,
        sk_prefix: &str,
    ) -> DaoResult<Vec<T>> {
        let pk = Pk::Series(series_id.into()).to_string();
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

    /// Bump `occurrences_generated` from `expected` to `expected + 1`,
    /// guarded on it still reading `expected` — the same "expected old
    /// value" idempotency guard as e.g. `Dao::put_rating`'s
    /// `rating_update`. `materialize_series` calls this once per occurrence
    /// it generates, with `expected` = that occurrence's 0-based index (the
    /// count already generated before it). A retry of the same activity
    /// (redelivery, or a resumed workflow replaying past an occurrence it
    /// already created) calls this again with the same `expected`; by then
    /// the stored value has moved on, so the condition fails — treated as
    /// success, not a double count, since the bump this call wanted has
    /// already happened.
    #[tracing::instrument(skip(self))]
    pub async fn bump_series_occurrences_generated(
        &self,
        series_id: &str,
        expected: u32,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET occurrences_generated = :new")
            .condition_expression("attribute_exists(#pk) AND occurrences_generated = :expected")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":new", AttributeValue::N((expected + 1).to_string()))
            .expression_attribute_values(":expected", AttributeValue::N(expected.to_string()))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Update a series' template fields — `name`/`description`/`location`,
    /// the subset `PATCH /matches/:id`'s `scope: this_and_future`/`series`
    /// can reach — so occurrences `materialize_series` generates later pick
    /// up the same edit the caller made to existing occurrences. `None`
    /// leaves a field unchanged, same convention as `update_match_meta`. A
    /// no-op (all `None`) skips the write entirely.
    #[tracing::instrument(skip(self, location))]
    pub async fn update_series_template(
        &self,
        series_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        location: Option<LocationRecord>,
    ) -> DaoResult<()> {
        let mut set: Vec<String> = Vec::new();
        let mut names: std::collections::HashMap<String, String> = Default::default();
        let mut values: std::collections::HashMap<String, AttributeValue> = Default::default();

        if let Some(v) = name {
            set.push("#name = :name".into());
            names.insert("#name".into(), "name".into());
            values.insert(":name".into(), s(v));
        }
        if let Some(v) = description {
            set.push("description = :desc".into());
            values.insert(":desc".into(), s(v));
        }
        if let Some(loc) = location {
            set.push("#loc = :loc".into());
            names.insert("#loc".into(), "location".into());
            values.insert(":loc".into(), serde_dynamo::to_attribute_value(&loc)?);
        }

        if set.is_empty() {
            return Ok(());
        }
        names.insert("#pk".into(), ATTR_PK.into());

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression(format!("SET {}", set.join(", ")))
            .condition_expression("attribute_exists(#pk)")
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(Some(values))
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("series {series_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Move a series from status `from` to `to`, guarded on it still reading
    /// `from` — same idempotency role as
    /// [`Self::bump_series_occurrences_generated`]. `materialize_series`
    /// uses this once, at the end of a successful run, to flip
    /// `"generating"` → `"active"`; a replay/retry of that final step finds
    /// the status already moved on and is a no-op success rather than an
    /// error.
    #[tracing::instrument(skip(self))]
    pub async fn update_series_status(
        &self,
        series_id: &str,
        from: &str,
        to: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET #status = :to")
            .condition_expression("attribute_exists(#pk) AND #status = :from")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":to", s(to))
            .expression_attribute_values(":from", s(from))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(
                se.err(),
                PutItemError::ConditionalCheckFailedException(_)
            )
    )
}
