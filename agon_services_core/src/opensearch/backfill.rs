//! Shared implementation behind `agon_worker backfill-opensearch` and the
//! admin panel's `POST /admin/opensearch/backfill`: rebuild every match's and
//! user's OpenSearch documents from DynamoDB.
//!
//! Use it:
//! - **once, to load existing history** after OpenSearch is first deployed;
//! - **after every schema change** (a bumped index version): deploy the new
//!   worker first, so live changes already go to the new index, then run this
//!   to fill it. When it finishes cleanly it points the read aliases at the new
//!   version;
//! - **as a repair tool**, any time. It's idempotent: each match or user is
//!   rebuilt from the current DynamoDB state, so re-running only rewrites the
//!   same documents.
//!
//! It lists matches and users from the entity index (GSI4, `ENTITY#match` /
//! `ENTITY#user`, oldest first), so it reads only their keys rather than
//! scanning the table. Both run in the same invocation, each with its own
//! bounded concurrency, retries and resumable cursor, logged after every page.
//!
//! **Run the entity-index migration first** on a table that predates GSI4
//! (`claude/entity-index-migration`). A match or user without index keys isn't
//! listed, so it would be skipped silently. `--match <id>` doesn't use the
//! index, and targets matches only (no analogous per-user flag yet).
//!
//! `run`'s caller supplies an `on_progress` callback, invoked after every page
//! (and once more at the end) with the counts so far — the CLI (`agon_worker
//! backfill-opensearch`) ignores it, already logging via `tracing`; the admin
//! endpoint uses it to keep a pollable job status up to date.

use std::time::Duration;

use crate::dao::Dao;
use crate::dao::entity_index::EntityKind;
use crate::opensearch::{
    OpenSearchClient, SyncOutcome, UserSyncOutcome, all_indices, sync_match, sync_user,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct BackfillArgs {
    pub concurrency: usize,
    pub page_size: u32,
    pub resume: Option<String>,
    pub resume_users: Option<String>,
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
            resume_users: None,
            only: Vec::new(),
            swap: true,
            delete_old: false,
        }
    }
}

impl BackfillArgs {
    /// Parse `agon_worker backfill-opensearch`'s own CLI flags.
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
                "--resume-users" => out.resume_users = Some(value("--resume-users")?),
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

/// Counts so far, reported to `run`'s `on_progress` callback after every page
/// (and once more, complete, right before `run` returns).
#[derive(Debug, Clone, Default)]
pub struct BackfillProgress {
    pub match_pages: u64,
    pub matches_indexed: u64,
    pub matches_removed: u64,
    pub matches_player_rows: u64,
    pub matches_failed: u64,
    pub user_pages: u64,
    pub users_indexed: u64,
    pub users_removed: u64,
    pub users_failed: u64,
    /// Set on the final call, once both phases (and the alias swap, if
    /// requested) have finished.
    pub done: bool,
}

#[derive(Debug, Default)]
struct Counts {
    indexed: u64,
    removed: u64,
    player_rows: u64,
    failed: u64,
}

#[derive(Debug, Default)]
struct UserCounts {
    indexed: u64,
    removed: u64,
    failed: u64,
}

impl BackfillProgress {
    fn from_counts(match_pages: u64, c: &Counts, user_pages: u64, u: &UserCounts) -> Self {
        Self {
            match_pages,
            matches_indexed: c.indexed,
            matches_removed: c.removed,
            matches_player_rows: c.player_rows,
            matches_failed: c.failed,
            user_pages,
            users_indexed: u.indexed,
            users_removed: u.removed,
            users_failed: u.failed,
            done: false,
        }
    }
}

/// Run the backfill. Returns the process exit code (0 success, 1 a match/user
/// failed or the alias swap failed, 2 misconfiguration). `on_progress` is
/// called after every page of each phase, and once more (with `done: true`)
/// right before returning.
pub async fn run(
    args: BackfillArgs,
    on_progress: std::sync::Arc<dyn Fn(BackfillProgress) + Send + Sync>,
) -> i32 {
    let os = match OpenSearchClient::from_env() {
        Ok(Some(os)) => os,
        Ok(None) => {
            tracing::error!("OPENSEARCH_URL is not set; nothing to backfill into");
            return 2;
        }
        Err(e) => {
            tracing::error!(error = %e, "invalid OpenSearch settings");
            return 2;
        }
    };
    let Ok(table) = std::env::var("AGON_TABLE_NAME") else {
        tracing::error!("AGON_TABLE_NAME is not set");
        return 2;
    };
    // Say which DynamoDB this reads, so a run against a real table is never a
    // surprise. The SDK takes the endpoint from these same variables.
    let endpoint = std::env::var("AWS_ENDPOINT_URL_DYNAMODB")
        .or_else(|_| std::env::var("AWS_ENDPOINT_URL"))
        .unwrap_or_else(|_| "AWS (no endpoint override)".into());
    tracing::info!(table = %table, endpoint = %endpoint, "reading dynamodb");
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
    run_against(&os, Dao::from_env(table).await, args, on_progress).await
}

/// `run`, against an already-built `Dao`/`OpenSearchClient` pair — split out
/// so a caller that already holds both (the admin endpoint, reusing the
/// request-scoped clients) doesn't pay for building fresh ones.
pub async fn run_against(
    os: &OpenSearchClient,
    dao: Dao,
    args: BackfillArgs,
    on_progress: std::sync::Arc<dyn Fn(BackfillProgress) + Send + Sync>,
) -> i32 {
    let now = chrono::Utc::now().to_rfc3339();
    let mut counts = Counts::default();
    let user_counts = UserCounts::default();

    if !args.only.is_empty() {
        // Targeted repair: just these matches, no alias changes.
        for id in &args.only {
            record(&mut counts, id, sync_with_retry(os, &dao, id, &now).await);
        }
        log_summary(&counts);
        let mut progress = BackfillProgress::from_counts(1, &counts, 0, &user_counts);
        progress.done = true;
        on_progress(progress);
        return if counts.failed == 0 { 0 } else { 1 };
    }

    let mut cursor = args.resume.clone();
    let mut pages = 0u64;
    loop {
        let page = match list_with_retry(&dao, cursor.as_deref(), args.page_size).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, resume = ?cursor, "listing matches failed; re-run with --resume <cursor> to continue");
                log_summary(&counts);
                return 1;
            }
        };
        let results: Vec<_> = futures::stream::iter(page.items)
            .map(|id| {
                let (os, dao, now) = (&os, &dao, &now);
                async move { (id.clone(), sync_with_retry(os, dao, &id, now).await) }
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
        on_progress(BackfillProgress::from_counts(
            pages,
            &counts,
            0,
            &user_counts,
        ));
        if cursor.is_none() {
            break;
        }
    }

    let mut user_counts = user_counts;
    let mut user_cursor = args.resume_users.clone();
    let mut user_pages = 0u64;
    loop {
        let page = match list_users_with_retry(&dao, user_cursor.as_deref(), args.page_size).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, resume = ?user_cursor, "listing users failed; re-run with --resume-users <cursor> to continue");
                log_user_summary(&user_counts);
                return 1;
            }
        };
        let results: Vec<_> = futures::stream::iter(page.items)
            .map(|id| {
                let (os, dao, now) = (&os, &dao, &now);
                async move { (id.clone(), user_sync_with_retry(os, dao, &id, now).await) }
            })
            .buffer_unordered(args.concurrency)
            .collect()
            .await;
        for (id, res) in results {
            record_user(&mut user_counts, &id, res);
        }
        user_pages += 1;
        user_cursor = page.next_cursor;
        tracing::info!(
            pages = user_pages,
            indexed = user_counts.indexed,
            removed = user_counts.removed,
            failed = user_counts.failed,
            "user backfill progress"
        );
        on_progress(BackfillProgress::from_counts(
            pages,
            &counts,
            user_pages,
            &user_counts,
        ));
        if user_cursor.is_none() {
            break;
        }
    }

    for def in all_indices() {
        if let Err(e) = os.refresh(&os.write_index(&def)).await {
            tracing::warn!(error = %e, "refresh failed");
        }
    }
    log_summary(&counts);
    log_user_summary(&user_counts);

    if counts.failed > 0 || user_counts.failed > 0 {
        tracing::error!(
            matches_failed = counts.failed,
            users_failed = user_counts.failed,
            "some matches or users failed; aliases NOT moved. Fix the cause and re-run (it's safe to repeat)"
        );
        let mut progress = BackfillProgress::from_counts(pages, &counts, user_pages, &user_counts);
        progress.done = true;
        on_progress(progress);
        return 1;
    }
    if args.swap {
        for def in all_indices() {
            let alias = def.alias(os.prefix());
            let previous = os.alias_targets(&alias).await.unwrap_or_default();
            if let Err(e) = os.point_alias(&def).await {
                tracing::error!(alias = %alias, error = %e, "could not move alias");
                let mut progress =
                    BackfillProgress::from_counts(pages, &counts, user_pages, &user_counts);
                progress.done = true;
                on_progress(progress);
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
    let mut progress = BackfillProgress::from_counts(pages, &counts, user_pages, &user_counts);
    progress.done = true;
    on_progress(progress);
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
        "match backfill finished"
    );
}

fn record_user(counts: &mut UserCounts, id: &str, res: Result<UserSyncOutcome, String>) {
    match res {
        Ok(UserSyncOutcome::Indexed) => counts.indexed += 1,
        Ok(UserSyncOutcome::Removed) => counts.removed += 1,
        Err(e) => {
            counts.failed += 1;
            tracing::error!(user_id = %id, error = %e, "user failed after retries");
        }
    }
}

fn log_user_summary(c: &UserCounts) {
    tracing::info!(
        indexed = c.indexed,
        removed = c.removed,
        failed = c.failed,
        "user backfill finished"
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

async fn list_with_retry(
    dao: &Dao,
    cursor: Option<&str>,
    page_size: u32,
) -> Result<crate::dao::Page<String>, String> {
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match dao
            .list_entity_ids(EntityKind::Match, false, cursor, page_size)
            .await
        {
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

/// [`sync_with_retry`], for users.
async fn user_sync_with_retry(
    os: &OpenSearchClient,
    dao: &Dao,
    id: &str,
    now: &str,
) -> Result<UserSyncOutcome, String> {
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match sync_user(os, dao, id, now, false).await {
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

/// [`list_with_retry`], for users.
async fn list_users_with_retry(
    dao: &Dao,
    cursor: Option<&str>,
    page_size: u32,
) -> Result<crate::dao::Page<String>, String> {
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match dao
            .list_entity_ids(EntityKind::User, false, cursor, page_size)
            .await
        {
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

/// An error from one of the page-level primitives below (used by the
/// Temporal backfill workflow's activities — see
/// `crate::opensearch::backfill_workflow`). Plain string-wrapped, same as
/// this module's existing retry helpers, just given a real `Error` impl so
/// it converts into Temporal's `ActivityError` via its blanket `From`.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct BackfillError(pub String);

impl From<String> for BackfillError {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// One page's outcome from [`backfill_match_page`]/[`backfill_user_page`].
/// `player_rows` is always 0 for the user phase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageOutcome {
    pub indexed: u64,
    pub removed: u64,
    pub player_rows: u64,
    pub failed: u64,
    pub next_cursor: Option<String>,
}

/// Checks OpenSearch is reachable and every index exists, creating what's
/// missing — the same preflight `run` does before paging. Called once at the
/// start of the Temporal workflow.
pub async fn ensure_ready(os: &OpenSearchClient) -> Result<(), BackfillError> {
    os.health()
        .await
        .map_err(|e| BackfillError(e.to_string()))?;
    for def in all_indices() {
        os.ensure_index(&def)
            .await
            .map_err(|e| BackfillError(e.to_string()))?;
    }
    Ok(())
}

/// Sync one page of matches (bounded `concurrency` fan-out, same per-item
/// retry as `run`). Used by the Temporal workflow's per-page activity —
/// each page is its own activity invocation, so the workflow's own history
/// durably records progress and a crash resumes from the last completed
/// page's `next_cursor` rather than from the top.
pub async fn backfill_match_page(
    os: &OpenSearchClient,
    dao: &Dao,
    cursor: Option<String>,
    page_size: u32,
    concurrency: usize,
) -> Result<PageOutcome, BackfillError> {
    let now = chrono::Utc::now().to_rfc3339();
    let page = list_with_retry(dao, cursor.as_deref(), page_size)
        .await
        .map_err(BackfillError)?;
    let next_cursor = page.next_cursor;
    let results: Vec<_> = futures::stream::iter(page.items)
        .map(|id| {
            let (os, dao, now) = (os, dao, &now);
            async move { sync_with_retry(os, dao, &id, now).await }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;
    let mut out = PageOutcome {
        next_cursor,
        ..Default::default()
    };
    for res in results {
        match res {
            Ok(SyncOutcome::Indexed { player_rows }) => {
                out.indexed += 1;
                out.player_rows += player_rows as u64;
            }
            Ok(SyncOutcome::Removed) => out.removed += 1,
            Err(_) => out.failed += 1,
        }
    }
    Ok(out)
}

/// [`backfill_match_page`], for users.
pub async fn backfill_user_page(
    os: &OpenSearchClient,
    dao: &Dao,
    cursor: Option<String>,
    page_size: u32,
    concurrency: usize,
) -> Result<PageOutcome, BackfillError> {
    let now = chrono::Utc::now().to_rfc3339();
    let page = list_users_with_retry(dao, cursor.as_deref(), page_size)
        .await
        .map_err(BackfillError)?;
    let next_cursor = page.next_cursor;
    let results: Vec<_> = futures::stream::iter(page.items)
        .map(|id| {
            let (os, dao, now) = (os, dao, &now);
            async move { user_sync_with_retry(os, dao, &id, now).await }
        })
        .buffer_unordered(concurrency)
        .collect()
        .await;
    let mut out = PageOutcome {
        next_cursor,
        ..Default::default()
    };
    for res in results {
        match res {
            Ok(UserSyncOutcome::Indexed) => out.indexed += 1,
            Ok(UserSyncOutcome::Removed) => out.removed += 1,
            Err(_) => out.failed += 1,
        }
    }
    Ok(out)
}

/// Refreshes every index and, if `swap`, points each alias at this version
/// (optionally deleting the superseded one) — the same tail `run` runs once
/// both phases finish with no failures. Called once at the end of the
/// Temporal workflow.
pub async fn finish(
    os: &OpenSearchClient,
    swap: bool,
    delete_old: bool,
) -> Result<(), BackfillError> {
    for def in all_indices() {
        if let Err(e) = os.refresh(&os.write_index(&def)).await {
            tracing::warn!(error = %e, "refresh failed");
        }
    }
    if !swap {
        return Ok(());
    }
    for def in all_indices() {
        let alias = def.alias(os.prefix());
        let previous = os.alias_targets(&alias).await.unwrap_or_default();
        os.point_alias(&def)
            .await
            .map_err(|e| BackfillError(e.to_string()))?;
        tracing::info!(alias = %alias, index = %os.write_index(&def), ?previous, "alias points at this version");
        if delete_old {
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
    Ok(())
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
    fn parses_resume_and_delete_old() {
        let a =
            BackfillArgs::parse(&args(&["--resume-users", "cursor123", "--delete-old"])).unwrap();
        assert_eq!(a.resume_users, Some("cursor123".to_string()));
        assert!(a.delete_old);
    }

    #[test]
    fn rejects_unknown_and_invalid() {
        assert!(BackfillArgs::parse(&args(&["--nope"])).is_err());
        assert!(BackfillArgs::parse(&args(&["--concurrency", "0"])).is_err());
    }
}
