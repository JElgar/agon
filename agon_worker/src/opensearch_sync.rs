//! Sync one match or user to OpenSearch — the *only* place an OpenSearch
//! write happens.
//!
//! `handlers::opensearch::target_for` decides, from a stream event, whether
//! a sync is warranted and what it targets; [`sync`] does the actual
//! read-from-DynamoDB-write-to-OpenSearch work for that target. It runs as
//! the `Action::OpenSearchSync` case of `action_consumer::ActionConsumer`,
//! on the same `agon-actions` queue and retry settings as every other
//! action — there's nothing about this sync that needs a longer or shorter
//! window than the rest, so it doesn't get a queue of its own. Each attempt
//! rebuilds the documents from DynamoDB's *current* state, so a late or
//! duplicate attempt never writes stale data. See docs/opensearch.md.

use agon_services_core::dao::Dao;
use agon_services_core::opensearch::{OpenSearchClient, sync_match, sync_user};
use serde::{Deserialize, Serialize};

use crate::error::WorkerResult;

/// What a sync re-syncs: one match's documents or one user's `users` doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenSearchSyncTarget {
    Match(String),
    User(String),
}

/// Re-sync `target` to OpenSearch now, using `dao` to read current state.
pub async fn sync(
    target: &OpenSearchSyncTarget,
    dao: &Dao,
    opensearch: &OpenSearchClient,
) -> WorkerResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    match target {
        OpenSearchSyncTarget::Match(id) => {
            sync_match(opensearch, dao, id, &now, true).await?;
        }
        OpenSearchSyncTarget::User(id) => {
            sync_user(opensearch, dao, id, &now, true).await?;
        }
    }
    tracing::debug!(?target, "opensearch synced");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_round_trips_through_json() {
        let t = OpenSearchSyncTarget::User("u9".into());
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<OpenSearchSyncTarget>(&s).unwrap(), t);

        let t = OpenSearchSyncTarget::Match("m1".into());
        let s = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<OpenSearchSyncTarget>(&s).unwrap(), t);
    }
}
