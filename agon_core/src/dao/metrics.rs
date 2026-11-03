//! Raw wearable-metric batches (heart rate today; later GPS/cadence/...)
//! posted by a paired device during a match, plus the incremental heart-rate
//! summary derived from them.
//!
//! Unlike `StatContributionRecord`'s sport-specific `counters`, which
//! `Dao::reconcile_match_contribution` fully recomputes from the match's
//! confirmed score every time, a wearable metric arrives as a live stream
//! throughout the match itself — there's no "current state" to recompute
//! from, only an accumulating total. So the raw batch and its derived
//! summary are both written here, independently of the stats reconciler
//! (which only carries `heart_rate` forward — see its doc comment).

use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, item_sk, s, to_item};
use super::keys::{Pk, Sk};
use super::records::{MetricBatchRecord, MetricSampleRecord};

/// Type tag for a raw metric-batch item.
pub const TYPE_METRIC_BATCH: &str = "metric_batch";

/// The one metric name this DAO currently folds into a derived summary
/// (`StatContributionRecord::heart_rate`). Any other `metric` value is still
/// accepted and stored as a raw batch — it just has no derived summary yet.
pub const METRIC_HEART_RATE: &str = "heart_rate";

/// One sample to append, as the caller supplies it.
#[derive(Debug, Clone, Copy)]
pub struct NewMetricSample {
    /// Seconds after the batch's `started_at`.
    pub offset_s: i64,
    pub value: f64,
}

impl Dao {
    /// Append one batch of wearable-metric samples for a player. Always a
    /// plain append — unlike live-scoring events, samples never conflict
    /// with each other, so there's no `expected_last_seq`-style gate; a
    /// retry that resends the same `started_at` just overwrites the same
    /// item with the same content.
    ///
    /// For `metric == METRIC_HEART_RATE`, also folds the batch into this
    /// player's running `StatContributionRecord.heart_rate` — deliberately
    /// a second, best-effort write rather than bundled into one
    /// `TransactWriteItems` with the batch put: the max-bpm update below is
    /// itself conditional (it only applies when the batch actually beats
    /// the stored max) and a `TransactWriteItems` member failing its
    /// condition cancels the *whole* transaction, which would wrongly also
    /// drop the raw batch and the sum/count update whenever a batch simply
    /// doesn't contain a new max. Same reasoning as `update_best_figures`'s
    /// own best-effort update outside `reconcile_match_contribution`'s
    /// transaction.
    #[tracing::instrument(skip(self, samples))]
    pub async fn append_metric_batch(
        &self,
        match_id: &str,
        metric: &str,
        user_id: &str,
        started_at: &str,
        samples: &[NewMetricSample],
        recorded_at: &str,
    ) -> DaoResult<()> {
        if samples.is_empty() {
            return Err(DaoError::Malformed("no samples to append".into()));
        }

        let record = MetricBatchRecord {
            metric: metric.to_string(),
            user_id: user_id.to_string(),
            started_at: started_at.to_string(),
            samples: samples
                .iter()
                .map(|smp| MetricSampleRecord {
                    offset_s: smp.offset_s,
                    value: smp.value,
                })
                .collect(),
            recorded_at: recorded_at.to_string(),
        };
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::MetricBatch {
                metric: metric.into(),
                user_id: user_id.into(),
                started_at: started_at.into(),
            },
            TYPE_METRIC_BATCH,
            &record,
        )?;
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        if metric == METRIC_HEART_RATE {
            self.apply_heart_rate_batch(match_id, user_id, samples)
                .await?;
        }
        Ok(())
    }

    /// Fold one batch's samples into this player's running heart-rate
    /// contribution: an unconditional `ADD` on `sum_bpm`/`sample_count`
    /// (always correct, never fails), plus a best-effort conditional `SET`
    /// on `max_bpm` that only applies when the batch's own max beats the
    /// stored one — a `ConditionalCheckFailedException` there just means
    /// this batch didn't set a new high, not an error (same pattern as
    /// `update_best_figures`).
    ///
    /// Both updates create the `StatContributionRecord` item (and its
    /// `heart_rate` map) if it doesn't exist yet — routine the first time a
    /// watch posts heart-rate for a match that hasn't been reconciled yet
    /// (an in-progress, unconfirmed match has no contribution item at all
    /// otherwise). See `StatContributionRecord`'s doc comment for why its
    /// other fields default on deserialize to tolerate that.
    async fn apply_heart_rate_batch(
        &self,
        match_id: &str,
        user_id: &str,
        samples: &[NewMetricSample],
    ) -> DaoResult<()> {
        let pk = s(Pk::Match(match_id.into()).to_string());
        let sk = s(Sk::StatContribution(user_id.into()).to_string());

        let count = samples.len() as u64;
        let sum: f64 = samples.iter().map(|smp| smp.value).sum();
        // Samples are bpm, always >= 0, so truncating towards zero is exact
        // for any realistic value and matches `max_bpm`'s `u32` storage.
        let sum_bpm = sum.round() as u64;
        let batch_max = samples
            .iter()
            .map(|smp| smp.value.round() as u32)
            .max()
            .unwrap_or(0);

        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, pk.clone())
            .key(ATTR_SK, sk.clone())
            .update_expression(
                "ADD heart_rate.sum_bpm :sum, heart_rate.sample_count :count \
                 SET heart_rate.max_bpm = if_not_exists(heart_rate.max_bpm, :zero)",
            )
            .expression_attribute_values(":sum", AttributeValue::N(sum_bpm.to_string()))
            .expression_attribute_values(":count", AttributeValue::N(count.to_string()))
            .expression_attribute_values(":zero", AttributeValue::N("0".to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, pk)
            .key(ATTR_SK, sk)
            .update_expression("SET heart_rate.max_bpm = :new")
            .condition_expression("heart_rate.max_bpm < :new")
            .expression_attribute_values(":new", AttributeValue::N(batch_max.to_string()))
            .send()
            .await;
        match result {
            Ok(_) => {}
            Err(e) if is_update_conditional_failure(&e) => {}
            Err(e) => return Err(DaoError::Dynamo(e.to_string())),
        }
        Ok(())
    }

    /// One player's batches for one metric, oldest first — a full trace
    /// (e.g. a heart-rate graph), not a paginated list: a single match's
    /// worth of batches is small (a few hundred at most at the agreed 30s
    /// flush cadence).
    #[tracing::instrument(skip(self))]
    pub async fn list_metric_batches(
        &self,
        match_id: &str,
        metric: &str,
        user_id: &str,
    ) -> DaoResult<Vec<MetricBatchRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::metric_player_prefix(metric, user_id)))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut batches = Vec::new();
        for item in out.items.unwrap_or_default() {
            // Guard against a hypothetical future SK that merely shares this
            // prefix (mirrors the care `list_stat_contribution_user_ids`
            // takes) — parse the key before trusting the item's shape.
            if let Sk::MetricBatch { .. } = item_sk(&item)? {
                batches.push(from_item(item)?);
            }
        }
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_sample_type_tag_is_stable() {
        // Pinned so a future rename is a deliberate, reviewed change — the
        // `type` attribute is read back by nothing today, but other `TYPE_*`
        // tags in this DAO are relied on by debugging/ops queries.
        assert_eq!(TYPE_METRIC_BATCH, "metric_batch");
        assert_eq!(METRIC_HEART_RATE, "heart_rate");
    }
}
