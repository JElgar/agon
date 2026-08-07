//! Standalone, locally-runnable migration: embeds every match's sides onto
//! its `#META` record (see `agon_core::dao::migrate` for the actual DAO work
//! this drives). Run once against the real table after the embedded-`sides`
//! deploy lands, before removing the code paths that still know how to read
//! standalone `SIDE#` items.
//!
//! ```text
//! # Preview only — no writes.
//! AGON_TABLE_NAME=agon cargo run -p agon_core --bin migrate_sides -- --dry-run
//!
//! # Actually migrate (embeds `sides`, leaves the old SIDE# items in place).
//! AGON_TABLE_NAME=agon cargo run -p agon_core --bin migrate_sides
//!
//! # Migrate and clean up the now-redundant SIDE# items. Run this only after
//! # a plain run above looks right — it's the one irreversible step.
//! AGON_TABLE_NAME=agon cargo run -p agon_core --bin migrate_sides -- --delete-old-items
//! ```
//!
//! Uses whatever AWS credentials are ambient in the shell (env vars / profile
//! / SSO) via `aws_config::load_from_env`, same as the service and worker —
//! point it at the real table by exporting `AGON_TABLE_NAME` (and the usual
//! `AWS_*`/`AWS_PROFILE` vars) before running.

use agon_core::dao::Dao;

#[tokio::main]
async fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");
    let delete_old_items = std::env::args().any(|a| a == "--delete-old-items");

    if dry_run && delete_old_items {
        eprintln!("--dry-run and --delete-old-items are mutually exclusive");
        std::process::exit(2);
    }

    let table_name = std::env::var("AGON_TABLE_NAME").unwrap_or_else(|_| "agon".to_string());
    println!(
        "migrate_sides: table={table_name} dry_run={dry_run} delete_old_items={delete_old_items}"
    );

    let dao = Dao::from_env(table_name).await;

    match dao.migrate_sides(dry_run, delete_old_items).await {
        Ok(summary) => {
            let verb = if dry_run { "would migrate" } else { "migrated" };
            println!(
                "{} {} match(es); {} already migrated; {} had no SIDE# items",
                verb,
                summary.migrated.len(),
                summary.already_migrated.len(),
                summary.no_sides.len(),
            );
            if !summary.migrated.is_empty() {
                println!("{verb}:");
                for id in &summary.migrated {
                    println!("  {id}");
                }
            }
            if !summary.no_sides.is_empty() {
                println!("no SIDE# items found (worth a look):");
                for id in &summary.no_sides {
                    println!("  {id}");
                }
            }
        }
        Err(e) => {
            eprintln!("migration failed: {e}");
            std::process::exit(1);
        }
    }
}
