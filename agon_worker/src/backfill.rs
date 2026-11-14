//! `agon_worker backfill-opensearch`: rebuild every match's OpenSearch documents
//! from DynamoDB.
//!
//! Use it:
//! - **once, to load existing history** after OpenSearch is first deployed;
//! - **after every schema change** (a bumped index version): deploy the new
//!   worker first, so live changes already go to the new index, then run this
//!   to fill it. When it finishes cleanly it points the read aliases at the new
//!   version;
//! - **as a repair tool**, any time. It's idempotent: each match is rebuilt from
//!   the current DynamoDB state, so re-running only rewrites the same documents.
//!
//! It scans the table (only `MATCH#…/#META` keys), syncs matches with bounded
//! concurrency and retries, and logs a resumable cursor after every page.
//!
//! ```text
//! agon_worker backfill-opensearch [--concurrency 8] [--page-size 500]
//!                                 [--resume <cursor>] [--match <id>]...
//!                                 [--no-swap] [--delete-old]
//! ```
//!
//! Needs `AGON_TABLE_NAME`, AWS credentials and `OPENSEARCH_URL` (plus the
//! optional `OPENSEARCH_USERNAME`, `OPENSEARCH_PASSWORD` and
//! `OPENSEARCH_INDEX_PREFIX`). It does not need the SQS, Meilisearch or Temporal
//! settings the worker needs.

use std::time::Duration;

use agon_core::dao::Dao;
use agon_core::opensearch::{OpenSearchClient, SyncOutcome, all_indices, sync_match};
use futures::StreamExt;

#[derive(Debug, Clone)]
pub struct BackfillArgs {
    pub concurrency: usize,
    pub page_size: i32,
    pub resume: Option<String>,
    pub only: Vec<String>,
    pub swap: bool,
    pub delete_old: bool,
}

impl Default for BackfillArgs {
    fn default() -> Self {
        Self {
            concurrency: 8,
            page_size: 500,
            resume: None,
            only: Vec::new(),
            swap: true,
            delete_old: false,
        }
    }
}

impl BackfillArgs {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut out = Self::default();
        let mut it = args.iter();
        while let Some(arg) = it.next() {
            let mut value = |name: &str| {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match arg.as_str() {
                "--concurrency" => {
                    out.concurrency = value("--concurrency")?
                        .parse()
                        .map_err(|_| "--concurrency must be a number")?;
                }
                "--page-size" => {
                    out.page_size = value("--page-size")?
                        .parse()
                        .map_err(|_| "--page-size must be a number")?;
                }
                "--resume" => out.resume = Some(value("--resume")?),
                "--match" => out.only.push(value("--match")?),
                "--no-swap" => out.swap = false,
                "--delete-old" => out.delete_old = true,
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if out.concurrency == 0 {
            return Err("--concurrency must be at least 1".into());
        }
        Ok(out)
    }
}

#[derive(Debug, Default)]
struct Counts {
    indexed: u64,
    removed: u64,
    player_rows: u64,
    failed: u64,
}

/// Run the backfill. Returns the process exit code.
pub async fn run(args: BackfillArgs) -> i32 {
    let Some(os) = OpenSearchClient::from_env() else {
        tracing::error!("OPENSEARCH_URL is not set; nothing to backfill into");
        return 2;
    };
    let Ok(table) = std::env::var("AGON_TABLE_NAME") else {
        tracing::error!("AGON_TABLE_NAME is not set");
        return 2;
    };
    match os.health().await {
        Ok(h) => {
            tracing::info!(status = %h["status"], cluster = %h["cluster_name"], "opensearch reachable")
        }
        Err(e) => {
            tracing::error!(error = %e, "opensearch unreachable");
            return 1;
        }
    }
    for def in all_indices() {
        match os.ensure_index(&def).await {
            Ok(created) => tracing::info!(index = %os.write_index(&def), created, "index ready"),
            Err(e) => {
                tracing::error!(index = %os.write_index(&def), error = %e, "could not create index");
                return 1;
            }
        }
    }
    let dao = Dao::from_env(table).await;
    let now = chrono::Utc::now().to_rfc3339();
    let mut counts = Counts::default();

    if !args.only.is_empty() {
        // Targeted repair: just these matches, no alias changes.
        for id in &args.only {
            record(&mut counts, id, sync_with_retry(&os, &dao, id, &now).await);
        }
        log_summary(&counts);
        return if counts.failed == 0 { 0 } else { 1 };
    }

    let mut cursor = args.resume.clone();
    let mut pages = 0u64;
    loop {
        let page = match scan_with_retry(&dao, cursor.as_deref(), args.page_size).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, resume = ?cursor, "scan failed; re-run with --resume <cursor> to continue");
                log_summary(&counts);
                return 1;
            }
        };
        let results: Vec<_> = futures::stream::iter(page.items.iter())
            .map(|id| {
                let (os, dao, now) = (&os, &dao, &now);
                async move { (id.clone(), sync_with_retry(os, dao, id, now).await) }
            })
            .buffer_unordered(args.concurrency)
            .collect()
            .await;
        for (id, res) in results {
            record(&mut counts, &id, res);
        }
        pages += 1;
        cursor = page.next_cursor;
        tracing::info!(
            pages,
            indexed = counts.indexed,
            removed = counts.removed,
            player_rows = counts.player_rows,
            failed = counts.failed,
            resume = ?cursor,
            "backfill progress"
        );
        if cursor.is_none() {
            break;
        }
    }

    for def in all_indices() {
        if let Err(e) = os.refresh(&os.write_index(&def)).await {
            tracing::warn!(error = %e, "refresh failed");
        }
    }
    log_summary(&counts);

    if counts.failed > 0 {
        tracing::error!(
            failed = counts.failed,
            "some matches failed; aliases NOT moved. Fix the cause and re-run (it's safe to repeat)"
        );
        return 1;
    }
    if args.swap {
        for def in all_indices() {
            let alias = def.alias(os.prefix());
            let previous = os.alias_targets(&alias).await.unwrap_or_default();
            if let Err(e) = os.point_alias(&def).await {
                tracing::error!(alias = %alias, error = %e, "could not move alias");
                return 1;
            }
            tracing::info!(alias = %alias, index = %os.write_index(&def), ?previous, "alias points at this version");
            if args.delete_old {
                for old in previous.into_iter().filter(|i| *i != os.write_index(&def)) {
                    match os.delete_index(&old).await {
                        Ok(()) => tracing::info!(index = %old, "deleted superseded index"),
                        Err(e) => {
                            tracing::warn!(index = %old, error = %e, "could not delete superseded index")
                        }
                    }
                }
            }
        }
    }
    0
}

fn record(counts: &mut Counts, id: &str, res: Result<SyncOutcome, String>) {
    match res {
        Ok(SyncOutcome::Indexed { player_rows }) => {
            counts.indexed += 1;
            counts.player_rows += player_rows as u64;
        }
        Ok(SyncOutcome::Removed) => counts.removed += 1,
        Err(e) => {
            counts.failed += 1;
            tracing::error!(match_id = %id, error = %e, "match failed after retries");
        }
    }
}

fn log_summary(c: &Counts) {
    tracing::info!(
        indexed = c.indexed,
        removed = c.removed,
        player_rows = c.player_rows,
        failed = c.failed,
        "backfill finished"
    );
}

/// Retry transient failures (DynamoDB throttling, OpenSearch hiccups) with
/// exponential backoff: 0.5s, 1s, 2s, 4s.
async fn sync_with_retry(
    os: &OpenSearchClient,
    dao: &Dao,
    id: &str,
    now: &str,
) -> Result<SyncOutcome, String> {
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match sync_match(os, dao, id, now, false).await {
            Ok(o) => return Ok(o),
            Err(e) => {
                last = e.to_string();
                if attempt < 4 {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    Err(last)
}

async fn scan_with_retry(
    dao: &Dao,
    cursor: Option<&str>,
    page_size: i32,
) -> Result<agon_core::dao::Page<String>, String> {
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match dao.scan_match_ids(None, cursor, page_size).await {
            Ok(p) => return Ok(p),
            Err(e) => {
                last = e.to_string();
                if attempt < 4 {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags() {
        let a = BackfillArgs::parse(&args(&[
            "--concurrency",
            "4",
            "--no-swap",
            "--match",
            "m1",
            "--match",
            "m2",
        ]))
        .unwrap();
        assert_eq!(a.concurrency, 4);
        assert!(!a.swap);
        assert_eq!(a.only, vec!["m1", "m2"]);
        assert_eq!(a.page_size, 500);
    }

    #[test]
    fn rejects_unknown_and_zero_concurrency() {
        assert!(BackfillArgs::parse(&args(&["--nope"])).is_err());
        assert!(BackfillArgs::parse(&args(&["--concurrency", "0"])).is_err());
    }
}
