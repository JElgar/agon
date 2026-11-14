//! Bring OpenSearch in line with DynamoDB for one match. Shared by the worker's
//! stream handler (live changes) and the backfill (everything).
//!
//! Always rebuilds from the current DynamoDB state, so it's idempotent and
//! self-correcting: running it twice, or out of order, converges.

use serde_json::json;

use super::client::{BulkOp, OpenSearchClient, OpenSearchError};
use super::docs::{build_match_docs, player_doc_id};
use super::schema::{MATCH_STATS, MATCHES};
use crate::dao::Dao;
use crate::dao::error::DaoError;

/// More player rows than any one match can have; bounds the stale-row lookup.
const MAX_ROWS: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Dao(#[from] DaoError),
    #[error(transparent)]
    OpenSearch(#[from] OpenSearchError),
}

/// What happened to the match, for logging and backfill counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOutcome {
    /// Match document written, with this many player rows.
    Indexed { player_rows: usize },
    /// The match no longer exists; its documents were removed.
    Removed,
}

/// Rebuild `match_id`'s documents in this build's physical indices.
///
/// 1. Read the match (and its tournament, if any) from DynamoDB.
/// 2. Index the match document and every current player row in one bulk call.
/// 3. Find this match's player rows that no longer apply (a player removed from
///    the roster, or the score un-confirmed) and delete them.
///
/// Step 3 runs after step 2, so a reader never sees the match with no rows.
///
/// `wait_for_refresh`: true on the live path, so the writes are searchable by
/// the time a later change to the same match looks for stale rows. The backfill
/// passes false (it never revisits a match within a second).
pub async fn sync_match(
    os: &OpenSearchClient,
    dao: &Dao,
    match_id: &str,
    now: &str,
    wait_for_refresh: bool,
) -> Result<SyncOutcome, SyncError> {
    let matches_index = os.write_index(&MATCHES);
    let stats_index = os.write_index(&MATCH_STATS);

    let Some(agg) = dao.get_match(match_id).await? else {
        let mut ops = vec![BulkOp::Delete {
            index: matches_index,
            id: match_id.to_string(),
        }];
        for id in os
            .ids_matching(
                &stats_index,
                json!({ "term": { "match_id": match_id } }),
                MAX_ROWS,
            )
            .await?
        {
            ops.push(BulkOp::Delete {
                index: stats_index.clone(),
                id,
            });
        }
        os.bulk_with(&ops, wait_for_refresh).await?;
        return Ok(SyncOutcome::Removed);
    };

    let tournament = match &agg.match_.tournament_id {
        Some(tid) => dao.get_tournament(tid).await?,
        None => None,
    };
    let docs = build_match_docs(&agg, tournament.as_ref(), now);

    let mut ops = Vec::with_capacity(1 + docs.players.len());
    ops.push(BulkOp::index(&matches_index, match_id, &docs.match_doc)?);
    let mut keep = Vec::with_capacity(docs.players.len());
    for row in &docs.players {
        let id = player_doc_id(&row.match_id, &row.user_id);
        ops.push(BulkOp::index(&stats_index, &id, row)?);
        keep.push(id);
    }
    os.bulk_with(&ops, wait_for_refresh).await?;

    let stale: Vec<BulkOp> = os
        .ids_matching(
            &stats_index,
            json!({ "term": { "match_id": match_id } }),
            MAX_ROWS,
        )
        .await?
        .into_iter()
        .filter(|id| !keep.contains(id))
        .map(|id| BulkOp::Delete {
            index: stats_index.clone(),
            id,
        })
        .collect();
    os.bulk_with(&stale, wait_for_refresh).await?;

    Ok(SyncOutcome::Indexed {
        player_rows: docs.players.len(),
    })
}
