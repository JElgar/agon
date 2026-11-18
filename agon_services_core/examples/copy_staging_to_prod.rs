//! One-off copy of the "real" (non-test) users and matches from the
//! `staging` DynamoDB table into `prod`, so prod doesn't start empty when
//! staging already holds real history (it was the only environment before
//! `prod` existed). Not wired into any binary or CI job — run by hand, once,
//! same pattern as the entity-index migration script
//! (`claude/entity-index-migration`).
//!
//! ```bash
//! cargo run -p agon_services_core --example copy_staging_to_prod -- \
//!     --from agon-staging --to agon-prod --dry-run
//! # review the summary, then drop --dry-run to actually write
//! ```
//!
//! Needs ambient AWS credentials (env/profile) with read access to `--from`
//! and write access to `--to` — same account, since both tables live under
//! one AWS account today (just different table names per Pulumi stack).
//!
//! ## What "real" means
//!
//! James's rule (2026-11-18): a user or match is "test" if the word "test"
//! appears in its name (case-insensitive) — everything else is real and
//! gets copied.
//!
//! ## What actually gets copied, and why
//!
//! A "user" or "match" isn't one DynamoDB item, it's a whole item
//! collection (everything sharing that `USER#`/`MATCH#` partition key) —
//! see `agon_services_core::dao::keys`. This copies entire partitions for
//! every kept id, not just the `#PROFILE`/`#META` item, which carries over
//! followers, ratings, stat contributions, comments, likes, live-scoring
//! events, etc. as a side effect of copying by partition rather than by
//! item type.
//!
//! Three things outside a kept user/match's own partition also have to
//! come along:
//! - **Teams a kept match's sides reference** (`MatchSideRecord::team_id`)
//!   — without the team's own partition, the match would point at a team
//!   that doesn't exist in prod. Copied regardless of the team's own name,
//!   since it's a hard dependency of a match we were told is real.
//! - **The `EMAIL#`/`AUTH#` guard items for a kept user** — these live
//!   under their own partition keys (`EMAIL#<email>`, `AUTH#<sub>`), not
//!   under `USER#<uid>`, so copying the user's partition alone would leave
//!   them unable to log in. This only works because prod shares staging's
//!   Supabase project (James, 2026-11-17) — the same `sub` is valid in
//!   both, so the copied `AUTH#<sub>` guard resolves correctly in prod.
//! - A kept user's own fan-out feed (`UFEED#<uid>`) — copied for
//!   completeness; harmless if it's regenerated anyway (see below).
//!
//! **Deliberately NOT copied**: invitations, join links, assets,
//! device-pairing codes, series, tournaments. None of today's "real"
//! matches reference a series or tournament (the script warns loudly if
//! that assumption turns out wrong — see `series_id` below); the rest are
//! either ephemeral or meaningless across environments (a join link/asset
//! upload is tied to this environment's own URLs).
//!
//! ## Why re-running the async pipeline on these items is safe
//!
//! Writing these items into `prod`'s table fires `prod`'s own DynamoDB
//! Streams → EventBridge Pipe → SQS → `agon_worker`, exactly as if they'd
//! just been created there:
//! - `FanOutMatch` re-fires for every copied match — intended, this is what
//!   (re)populates the copied users' feeds in prod.
//! - The stats/rating reconcilers (`Dao::apply_stat_contribution` /
//!   `apply_rating_contribution`) diff the match's (copied, final) desired
//!   contribution against the (also copied, already-final)
//!   `STATCONTRIB#`/`RATINGCONTRIB#` item and find them equal → zero delta,
//!   no double-counting (see their doc comments: "idempotent: unchanged
//!   state → zero delta → no write"). This is *why* those items are copied
//!   alongside the match rather than left for the reconciler to rebuild —
//!   leaving them out would make the first replay apply the full
//!   contribution a second time on top of profile stats that already
//!   include it.
//! - `MatchReminder` only auto-starts for a match with `series_id` set — see
//!   the warning below.
//!
//! ## Known gap: images
//!
//! A copied user/team/match's image URL (if any) still points at
//! *staging's* CloudFront domain (`AGON_ASSETS_CDN_URL` differs per stack).
//! Public images (profile/team) will keep loading fine cross-environment;
//! a signed `match_header` URL's signature will simply have expired by the
//! time this runs. Re-uploading those images in prod, if it matters, is a
//! separate manual step this script doesn't attempt.

use std::collections::{HashMap, HashSet};
use std::env;

use agon_services_core::dao::item::{item_pk, item_sk};
use agon_services_core::dao::keys::{Pk, Sk};
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::{AttributeValue, WriteRequest, PutRequest};

type Item = HashMap<String, AttributeValue>;

struct Args {
    from: String,
    to: String,
    dry_run: bool,
}

fn parse_args() -> Args {
    let args: Vec<String> = env::args().collect();
    let mut from = None;
    let mut to = None;
    let mut dry_run = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--from" => {
                from = args.get(i + 1).cloned();
                i += 2;
            }
            "--to" => {
                to = args.get(i + 1).cloned();
                i += 2;
            }
            "--dry-run" => {
                dry_run = true;
                i += 1;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let (Some(from), Some(to)) = (from, to) else {
        eprintln!(
            "usage: copy_staging_to_prod --from <source-table> --to <dest-table> [--dry-run]"
        );
        std::process::exit(2);
    };
    if from == to {
        eprintln!("--from and --to must be different tables");
        std::process::exit(2);
    }
    Args { from, to, dry_run }
}

/// Case-insensitive "does this item's `name` attribute contain 'test'" —
/// James's rule for what counts as a test user/match.
fn name_is_test(item: &Item) -> bool {
    item.get("name")
        .and_then(|v| v.as_s().ok())
        .map(|n| n.to_lowercase().contains("test"))
        .unwrap_or(false)
}

fn string_attr(item: &Item, key: &str) -> Option<String> {
    item.get(key).and_then(|v| v.as_s().ok()).cloned()
}

async fn scan_all(client: &Client, table: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let mut exclusive_start_key = None;
    loop {
        let mut req = client.scan().table_name(table);
        if let Some(key) = exclusive_start_key.take() {
            req = req.set_exclusive_start_key(Some(key));
        }
        let resp = req.send().await.expect("scan failed");
        items.extend(resp.items.unwrap_or_default());
        match resp.last_evaluated_key {
            Some(key) if !key.is_empty() => exclusive_start_key = Some(key),
            _ => break,
        }
    }
    items
}

/// Write `items` into `table` via `BatchWriteItem`, 25 at a time, retrying
/// `UnprocessedItems` with simple linear backoff.
async fn batch_write(client: &Client, table: &str, items: Vec<Item>) {
    for chunk in items.chunks(25) {
        let mut requests: Vec<WriteRequest> = chunk
            .iter()
            .map(|item| {
                WriteRequest::builder()
                    .put_request(
                        PutRequest::builder()
                            .set_item(Some(item.clone()))
                            .build()
                            .expect("put request"),
                    )
                    .build()
            })
            .collect();

        let mut attempt = 0;
        loop {
            let resp = client
                .batch_write_item()
                .request_items(table, requests.clone())
                .send()
                .await
                .expect("batch write failed");

            let unprocessed = resp
                .unprocessed_items
                .and_then(|mut m| m.remove(table))
                .unwrap_or_default();

            if unprocessed.is_empty() {
                break;
            }
            attempt += 1;
            if attempt > 8 {
                panic!("giving up on {} unprocessed items after 8 retries", unprocessed.len());
            }
            requests = unprocessed;
            tokio::time::sleep(std::time::Duration::from_millis(200 * attempt as u64)).await;
        }
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();

    let config = aws_config::load_from_env().await;
    let client = Client::new(&config);

    println!("scanning {} ...", args.from);
    let items = scan_all(&client, &args.from).await;
    println!("scanned {} items", items.len());

    // Pass 1: which users/matches are "real" (name doesn't contain "test").
    let mut users_keep: HashSet<String> = HashSet::new();
    let mut matches_keep: HashSet<String> = HashSet::new();
    let mut matches_with_series: Vec<String> = Vec::new();

    for item in &items {
        let (Ok(pk), Ok(sk)) = (item_pk(item), item_sk(item)) else {
            continue;
        };
        match (&pk, &sk) {
            (Pk::User(uid), Sk::Profile) if !name_is_test(item) => {
                users_keep.insert(uid.clone());
            }
            (Pk::Match(mid), Sk::Meta) if !name_is_test(item) => {
                matches_keep.insert(mid.clone());
                if string_attr(item, "series_id").is_some() {
                    matches_with_series.push(mid.clone());
                }
            }
            _ => {}
        }
    }

    // Pass 2: teams referenced by a kept match's sides — a hard dependency,
    // copied regardless of the team's own name.
    let mut teams_keep: HashSet<String> = HashSet::new();
    for item in &items {
        let (Ok(Pk::Match(mid)), Ok(Sk::Side(_))) = (item_pk(item), item_sk(item)) else {
            continue;
        };
        if matches_keep.contains(&mid)
            && let Some(team_id) = string_attr(item, "team_id")
        {
            teams_keep.insert(team_id);
        }
    }

    // Pass 3: decide what to actually copy.
    let mut to_copy: Vec<Item> = Vec::new();
    let mut skipped_other: HashMap<&'static str, u32> = HashMap::new();

    for item in items {
        let (Ok(pk), Ok(sk)) = (item_pk(&item), item_sk(&item)) else {
            *skipped_other.entry("unparseable key").or_default() += 1;
            continue;
        };
        let keep = match &pk {
            Pk::User(uid) => users_keep.contains(uid),
            Pk::Match(mid) => matches_keep.contains(mid),
            Pk::Team(tid) => teams_keep.contains(tid),
            Pk::UserFeed(uid) => users_keep.contains(uid),
            Pk::EmailGuard(_) | Pk::AuthGuard(_) if sk == Sk::Guard => string_attr(&item, "user_id")
                .map(|uid| users_keep.contains(&uid))
                .unwrap_or(false),
            Pk::Invitation(_) => {
                *skipped_other.entry("invitation").or_default() += 1;
                false
            }
            Pk::JoinLink(_) => {
                *skipped_other.entry("join link").or_default() += 1;
                false
            }
            Pk::Asset(_) => {
                *skipped_other.entry("asset").or_default() += 1;
                false
            }
            Pk::DevicePairing(_) => {
                *skipped_other.entry("device pairing").or_default() += 1;
                false
            }
            Pk::Series(_) => {
                *skipped_other.entry("series").or_default() += 1;
                false
            }
            Pk::Tournament(_) => {
                *skipped_other.entry("tournament").or_default() += 1;
                false
            }
            _ => false,
        };
        if keep {
            to_copy.push(item);
        }
    }

    println!();
    println!("users to copy:   {}", users_keep.len());
    println!("matches to copy: {}", matches_keep.len());
    println!("teams to copy (match dependency): {}", teams_keep.len());
    println!("total items to copy: {}", to_copy.len());
    if !skipped_other.is_empty() {
        println!();
        println!("skipped (not copied by design):");
        for (kind, count) in &skipped_other {
            println!("  {kind}: {count}");
        }
    }
    if !matches_with_series.is_empty() {
        println!();
        println!(
            "WARNING: {} kept match(es) have series_id set, but series items are never copied:",
            matches_with_series.len()
        );
        for mid in &matches_with_series {
            println!("  {mid}");
        }
        println!(
            "  Their series_id will dangle in prod, and re-running MaterializeSeries there needs the parent SERIES# item too — handle these separately."
        );
    }

    if args.dry_run {
        println!();
        println!("--dry-run: not writing anything to {}", args.to);
        return;
    }

    println!();
    println!("writing {} items to {} ...", to_copy.len(), args.to);
    batch_write(&client, &args.to, to_copy).await;
    println!("done.");
}
