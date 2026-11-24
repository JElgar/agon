//! One-off migration: give every existing team entry (`TOURNAMENT#<tid>` /
//! `ENTRY#<teamId>` in a `Teams`-audience tournament) its GSI1 projection
//! (`TEAMTOURNS#<teamId>` / `<entryCreatedAt>#<tid>`), so
//! `GET /teams/{id}/competitions` lists tournaments entered before the
//! projection existed. New entries get their keys on create
//! (`Dao::create_tournament_entry`), so this only needs to run once per
//! environment.
//!
//! **Lives on the `claude/team-competitions-migration` branch only, never on
//! `main`.** Run it from a checkout of that branch:
//!
//! ```text
//! # DynamoDB Local (endpoint override, no confirmation needed)
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret AGON_TABLE_NAME=agon \
//!   cargo run -p agon_services_core --example migrate_team_tournaments -- --dry-run
//!
//! # A real table: uses your normal AWS credentials, and needs --yes
//! AGON_TABLE_NAME=agon-staging cargo run -p agon_services_core --example migrate_team_tournaments -- --yes
//! ```
//!
//! Flags:
//! - `--dry-run`: count what would be updated, write nothing.
//! - `--yes`: required when there's no `AWS_ENDPOINT_URL` (i.e. a real table).
//! - `--concurrency N`: updates in flight at once. Default 4.
//! - `--page-size N`: items examined per scan page. Default 500.
//! - `--resume '<json>'`: continue from the cursor printed after each page.
//!
//! **Safe to re-run.** The scan only picks up `ENTRY#` items without a
//! `GSI1PK`, and each update is conditional on the item existing and still
//! having no `GSI1PK`, so it never creates an item and never rewrites one
//! that's done. Entries in a `Players`-audience tournament (the entrant is a
//! user) are skipped, as are entries whose tournament meta is missing; both
//! are counted as `not team` in the output, and a re-run sees them again
//! (they're cheap: one cached tournament read each). Throttling is retried
//! with backoff.
//!
//! Each update is a write, so it also produces a DynamoDB stream event. The
//! worker treats it like any other edit and finds nothing to change.

use std::collections::HashMap;
use std::time::Duration;

use agon_services_core::dao::keys::{Pk, Sk, team_tournaments_gsi1pk, team_tournaments_gsi1sk};
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::AttributeValue;

type Key = HashMap<String, AttributeValue>;

struct Args {
    dry_run: bool,
    yes: bool,
    concurrency: usize,
    page_size: i32,
    resume: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        dry_run: false,
        yes: false,
        concurrency: 4,
        page_size: 500,
        resume: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |n: &str| it.next().ok_or_else(|| format!("{n} needs a value"));
        match arg.as_str() {
            "--dry-run" => a.dry_run = true,
            "--yes" => a.yes = true,
            "--concurrency" => {
                a.concurrency = value("--concurrency")?
                    .parse()
                    .map_err(|_| "bad --concurrency")?
            }
            "--page-size" => {
                a.page_size = value("--page-size")?
                    .parse()
                    .map_err(|_| "bad --page-size")?
            }
            "--resume" => a.resume = Some(value("--resume")?),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if a.concurrency == 0 {
        return Err("--concurrency must be at least 1".into());
    }
    Ok(a)
}

/// An `ENTRY#` item without GSI1 keys.
#[derive(Clone)]
struct Missing {
    tournament_id: String,
    entrant_id: String,
    created_at: String,
}

fn s(v: impl Into<String>) -> AttributeValue {
    AttributeValue::S(v.into())
}

fn str_attr(item: &Key, name: &str) -> Option<String> {
    match item.get(name) {
        Some(AttributeValue::S(v)) => Some(v.clone()),
        _ => None,
    }
}

async fn scan_page(
    client: &Client,
    table: &str,
    start: Option<Key>,
    page_size: i32,
) -> Result<(Vec<Missing>, Option<Key>), String> {
    let out = client
        .scan()
        .table_name(table)
        .filter_expression(
            "attribute_not_exists(GSI1PK) AND begins_with(#pk, :tr) AND begins_with(#sk, :entry)",
        )
        .projection_expression("#pk, #sk, created_at")
        .expression_attribute_names("#pk", "PK")
        .expression_attribute_names("#sk", "SK")
        .expression_attribute_values(":tr", s(Pk::Tournament(String::new()).to_string()))
        .expression_attribute_values(":entry", s(Sk::entry_prefix()))
        .limit(page_size)
        .set_exclusive_start_key(start)
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;

    let mut found = Vec::new();
    for item in out.items.unwrap_or_default() {
        let (Some(pk), Some(sk)) = (str_attr(&item, "PK"), str_attr(&item, "SK")) else {
            continue;
        };
        let (Ok(Pk::Tournament(tournament_id)), Ok(Sk::Entry(entrant_id))) =
            (pk.parse::<Pk>(), sk.parse::<Sk>())
        else {
            continue;
        };
        found.push(Missing {
            tournament_id,
            entrant_id,
            created_at: str_attr(&item, "created_at").unwrap_or_default(),
        });
    }
    Ok((found, out.last_evaluated_key))
}

/// The tournament's `entry_audience` (`"teams"` | `"players"`), `None` if
/// the meta item is gone.
async fn audience(
    client: &Client,
    table: &str,
    tournament_id: &str,
) -> Result<Option<String>, String> {
    let out = client
        .get_item()
        .table_name(table)
        .key("PK", s(Pk::Tournament(tournament_id.into()).to_string()))
        .key("SK", s(Sk::Meta.to_string()))
        .projection_expression("entry_audience")
        .send()
        .await
        .map_err(|e| format!("{e:?}"))?;
    Ok(out.item.and_then(|i| str_attr(&i, "entry_audience")))
}

/// Set the keys on one entry. `Ok(false)` when it was already done or is gone.
async fn set_keys(client: &Client, table: &str, m: &Missing) -> Result<bool, String> {
    let res = client
        .update_item()
        .table_name(table)
        .key("PK", s(Pk::Tournament(m.tournament_id.clone()).to_string()))
        .key("SK", s(Sk::Entry(m.entrant_id.clone()).to_string()))
        .update_expression("SET GSI1PK = :pk1, GSI1SK = :sk1")
        .condition_expression("attribute_exists(PK) AND attribute_not_exists(GSI1PK)")
        .expression_attribute_values(":pk1", s(team_tournaments_gsi1pk(&m.entrant_id)))
        .expression_attribute_values(
            ":sk1",
            s(team_tournaments_gsi1sk(&m.created_at, &m.tournament_id)),
        )
        .send()
        .await;
    match res {
        Ok(_) => Ok(true),
        Err(e)
            if e.as_service_error()
                .is_some_and(|se| se.is_conditional_check_failed_exception()) =>
        {
            Ok(false)
        }
        Err(e) => Err(format!("{e:?}")),
    }
}

/// Retry transient failures (throttling) with backoff: 0.5s, 1s, 2s, 4s.
async fn retry<T, F, Fut>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let mut delay = Duration::from_millis(500);
    let mut last = String::new();
    for attempt in 0..5 {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = e;
                if attempt < 4 {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    Err(last)
}

fn encode_cursor(key: &Key) -> String {
    let v: serde_json::Value = serde_dynamo::from_item(key.clone()).expect("cursor encodes");
    v.to_string()
}

fn decode_cursor(c: &str) -> Result<Key, String> {
    let v: serde_json::Value = serde_json::from_str(c).map_err(|e| format!("bad --resume: {e}"))?;
    serde_dynamo::to_item(v).map_err(|e| format!("bad --resume: {e}"))
}

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let Ok(table) = std::env::var("AGON_TABLE_NAME") else {
        eprintln!("AGON_TABLE_NAME is not set");
        std::process::exit(2);
    };
    let endpoint = std::env::var("AWS_ENDPOINT_URL_DYNAMODB")
        .or_else(|_| std::env::var("AWS_ENDPOINT_URL"))
        .ok();
    println!(
        "table: {table}\nendpoint: {}\nmode: {}",
        endpoint.as_deref().unwrap_or("AWS (no endpoint override)"),
        if args.dry_run {
            "dry run (no writes)"
        } else {
            "write"
        }
    );
    if endpoint.is_none() && !args.dry_run && !args.yes {
        eprintln!("This is a real AWS table. Re-run with --yes to write (or --dry-run first).");
        std::process::exit(2);
    }

    let config = aws_config::load_from_env().await;
    let client = Client::new(&config);
    let mut start = match args.resume.as_deref().map(decode_cursor).transpose() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    // tournament id -> is a Teams-audience tournament.
    let mut audiences: HashMap<String, bool> = HashMap::new();
    let (mut found, mut not_team, mut written, mut skipped, mut failed, mut pages) =
        (0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    loop {
        let (items, next) =
            match retry(|| scan_page(&client, &table, start.clone(), args.page_size)).await {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("scan failed: {e}");
                    if let Some(k) = &start {
                        eprintln!("resume with: --resume '{}'", encode_cursor(k));
                    }
                    std::process::exit(1);
                }
            };

        let mut team_entries = Vec::new();
        for m in items {
            let is_team = match audiences.get(&m.tournament_id) {
                Some(t) => *t,
                None => match retry(|| audience(&client, &table, &m.tournament_id)).await {
                    Ok(a) => {
                        let t = a.as_deref() == Some("teams");
                        audiences.insert(m.tournament_id.clone(), t);
                        t
                    }
                    Err(e) => {
                        failed += 1;
                        eprintln!("failed reading tournament {}: {e}", m.tournament_id);
                        continue;
                    }
                },
            };
            if is_team {
                team_entries.push(m);
            } else {
                not_team += 1;
            }
        }
        found += team_entries.len() as u64;

        if !args.dry_run {
            for chunk in team_entries.chunks(args.concurrency) {
                let mut set = tokio::task::JoinSet::new();
                for m in chunk {
                    let (client, table, m) = (client.clone(), table.clone(), m.clone());
                    set.spawn(async move {
                        let res = retry(|| set_keys(&client, &table, &m)).await;
                        (m, res)
                    });
                }
                while let Some(joined) = set.join_next().await {
                    match joined {
                        Ok((_, Ok(true))) => written += 1,
                        Ok((_, Ok(false))) => skipped += 1,
                        Ok((m, Err(e))) => {
                            failed += 1;
                            eprintln!("failed {} / {}: {e}", m.tournament_id, m.entrant_id);
                        }
                        Err(e) => {
                            failed += 1;
                            eprintln!("task failed: {e}");
                        }
                    }
                }
            }
        }
        pages += 1;
        start = next;
        match &start {
            Some(k) => println!(
                "page {pages}: found {found}, not team {not_team}, written {written}, skipped {skipped}, failed {failed}; resume: --resume '{}'",
                encode_cursor(k)
            ),
            None => break,
        }
    }
    println!(
        "done: found {found}, not team {not_team}, written {written}, skipped {skipped}, failed {failed}"
    );
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
