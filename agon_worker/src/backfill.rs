//! `agon_worker backfill-opensearch`: CLI entry point over the shared
//! implementation in `agon_services_core::opensearch::backfill` (also used by
//! the admin panel's `POST /admin/opensearch/backfill`).
//!
//! ```text
//! agon_worker backfill-opensearch [--concurrency 8] [--page-size 500]
//!                                 [--resume <cursor>] [--resume-users <cursor>]
//!                                 [--match <id>]... [--no-swap] [--delete-old]
//! ```
//!
//! Needs `AGON_TABLE_NAME`, AWS credentials and `OPENSEARCH_URL` (plus the
//! optional `OPENSEARCH_USERNAME`, `OPENSEARCH_PASSWORD` and
//! `OPENSEARCH_INDEX_PREFIX`). It does not need the SQS or Temporal
//! settings the worker needs. Set `AWS_ENDPOINT_URL` to read DynamoDB Local
//! instead (`make backfill-opensearch-local`). It logs the table and endpoint
//! it reads (`reading dynamodb`) before it starts.

pub use agon_services_core::opensearch::backfill::BackfillArgs;

/// Run the backfill. Returns the process exit code. Progress is left to the
/// shared implementation's own `tracing` logging; the CLI has no separate
/// status to keep (unlike the admin endpoint, which polls `on_progress`).
pub async fn run(args: BackfillArgs) -> i32 {
    agon_services_core::opensearch::backfill::run(args, std::sync::Arc::new(|_| {})).await
}
