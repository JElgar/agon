//! `agon_worker migrate-entity-index`: give every existing match, user, team and
//! tournament its entity-index keys (GSI4), so they can be listed without a
//! table scan. See `agon_services_core::dao::entity_index`.
//!
//! New items get their keys when they're created, so this only matters for
//! items written before the index existed. Run it once per environment after
//! the index is deployed. It's idempotent: each update only touches an item that
//! exists and has no keys yet, so re-running is safe and cheap.
//!
//! ```text
//! agon_worker migrate-entity-index [--concurrency 4] [--page-size 500] [--resume <cursor>] [--dry-run]
//! ```
//!
//! Needs `AGON_TABLE_NAME` and AWS credentials (`AWS_ENDPOINT_URL` for DynamoDB
//! Local). It logs the table and endpoint before it starts.
//!
//! Each update is a write to the item, so it also produces a stream event. The
//! worker's handlers treat those like any other edit (re-index, re-reconcile)
//! and find nothing to change. Keep the concurrency modest on a busy table.

use std::time::Duration;

use agon_services_core::dao::Dao;
use agon_services_core::dao::entity_index::MissingEntityKeys;
use futures::StreamExt;

#[derive(Debug, Clone)]
pub struct MigrateArgs {
    pub concurrency: usize,
    pub page_size: i32,
    pub resume: Option<String>,
    pub dry_run: bool,
}

impl Default for MigrateArgs {
    fn default() -> Self {
        Self {
            concurrency: 4,
            page_size: 500,
            resume: None,
            dry_run: false,
        }
    }
}

impl MigrateArgs {
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
                "--dry-run" => out.dry_run = true,
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if out.concurrency == 0 {
            return Err("--concurrency must be at least 1".into());
        }
        Ok(out)
    }
}

/// Run the migration. Returns the process exit code.
pub async fn run(args: MigrateArgs) -> i32 {
    let Ok(table) = std::env::var("AGON_TABLE_NAME") else {
        tracing::error!("AGON_TABLE_NAME is not set");
        return 2;
    };
    let endpoint = std::env::var("AWS_ENDPOINT_URL_DYNAMODB")
        .or_else(|_| std::env::var("AWS_ENDPOINT_URL"))
        .unwrap_or_else(|_| "AWS (no endpoint override)".into());
    tracing::info!(table = %table, endpoint = %endpoint, dry_run = args.dry_run, "migrating entity index");
    let dao = Dao::from_env(table).await;

    let (mut found, mut written, mut skipped, mut failed) = (0u64, 0u64, 0u64, 0u64);
    let mut cursor = args.resume.clone();
    loop {
        let page = match scan_with_retry(&dao, cursor.as_deref(), args.page_size).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, resume = ?cursor, "scan failed; re-run with --resume <cursor> to continue");
                return 1;
            }
        };
        found += page.items.len() as u64;
        if !args.dry_run {
            let results: Vec<(MissingEntityKeys, Result<bool, String>)> =
                futures::stream::iter(page.items)
                    .map(|item| {
                        let dao = &dao;
                        async move {
                            let res = set_with_retry(dao, &item).await;
                            (item, res)
                        }
                    })
                    .buffer_unordered(args.concurrency)
                    .collect()
                    .await;
            for (item, res) in results {
                match res {
                    Ok(true) => written += 1,
                    Ok(false) => skipped += 1,
                    Err(e) => {
                        failed += 1;
                        tracing::error!(kind = item.kind.tag(), id = %item.id, error = %e, "could not set index keys");
                    }
                }
            }
        }
        cursor = page.next_cursor;
        tracing::info!(found, written, skipped, failed, resume = ?cursor, "migration progress");
        if cursor.is_none() {
            break;
        }
    }
    tracing::info!(found, written, skipped, failed, "migration finished");
    if failed == 0 { 0 } else { 1 }
}

async fn scan_with_retry(
    dao: &Dao,
    cursor: Option<&str>,
    page_size: i32,
) -> Result<agon_services_core::dao::Page<MissingEntityKeys>, String> {
    retry(|| dao.scan_entity_index_missing(cursor, page_size)).await
}

async fn set_with_retry(dao: &Dao, item: &MissingEntityKeys) -> Result<bool, String> {
    retry(|| dao.set_entity_index_keys(item)).await
}

/// Retry transient failures (throttling) with backoff: 0.5s, 1s, 2s, 4s.
async fn retry<T, E: std::fmt::Display, F, Fut>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match f().await {
            Ok(v) => return Ok(v),
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
        let a = MigrateArgs::parse(&args(&["--dry-run", "--concurrency", "2"])).unwrap();
        assert!(a.dry_run);
        assert_eq!(a.concurrency, 2);
        assert_eq!(a.page_size, 500);
    }

    #[test]
    fn rejects_unknown_flags() {
        assert!(MigrateArgs::parse(&args(&["--nope"])).is_err());
        assert!(MigrateArgs::parse(&args(&["--concurrency", "0"])).is_err());
    }
}
