//! Per-user, per-sport stats: list, and atomic increment on match completion.

use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, from_item, item_sk, s};
use super::keys::{Pk, Sk};
use super::records::UserSportStatsRecord;

impl Dao {
    /// List all of a user's per-sport stats (one item per sport). Unpaginated —
    /// a user only has a handful of sports.
    pub async fn list_user_stats(&self, user_id: &str) -> DaoResult<Vec<UserSportStatsRecord>> {
        let out = self
            .client
            .query()
            .table_name(self.table())
            .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":pk", s(Pk::User(user_id.into()).to_string()))
            .expression_attribute_values(":sk", s(Sk::Stats(String::new()).prefix()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut stats = Vec::new();
        for item in out.items.unwrap_or_default() {
            // Every item under this range is a stats item, but guard anyway.
            if let Sk::Stats(_) = item_sk(&item)? {
                stats.push(from_item(item)?);
            }
        }
        Ok(stats)
    }

    /// Record a played match for a user in a sport: increments `matches_played`
    /// and, if `won`, `wins`. Upserts the stats item (ADD treats a missing
    /// attribute as 0), so the first match for a sport creates the item.
    pub async fn record_match_result(
        &self,
        user_id: &str,
        sport: &str,
        won: bool,
    ) -> DaoResult<()> {
        // ADD both counters; also stamp match_type so the created item carries it.
        let win_delta = if won { 1 } else { 0 };
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::Stats(sport.into()).to_string()))
            .update_expression("ADD matches_played :one, wins :w SET match_type = :mt")
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .expression_attribute_values(":w", AttributeValue::N(win_delta.to_string()))
            .expression_attribute_values(":mt", s(sport))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}
