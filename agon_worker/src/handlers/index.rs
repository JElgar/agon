//! Inline handler: keep the Meilisearch indexes in sync with the table.
//!
//! Triggered by writes to a team meta item. On an
//! insert/modify we re-read the current record from DynamoDB and upsert a search
//! document; on a remove we delete the document by id. Re-reading (rather than
//! trusting a stream image) means we always index the latest committed state.
//!
//! The search documents are intentionally minimal — just what the discovery
//! endpoints need to match on and render a row. Full hydration happens from
//! DynamoDB when a result is opened.

use agon_services_core::dao::Dao;
use agon_services_core::dao::keys::{Pk, Sk};
use agon_services_core::dao::records::TeamRecord;
use serde::Serialize;

use crate::error::WorkerResult;
use crate::event::ChangeEvent;
use agon_services_core::search::{Index, SearchClient};

/// A team search document (index `teams`).
#[derive(Debug, Serialize)]
struct TeamDoc {
    id: String,
    name: String,
}

/// Handle an index-relevant change event. Returns `Ok(())` for events that are
/// not index-relevant (they're simply ignored — the router only routes the
/// relevant ones, but this stays total for safety).
pub async fn handle(dao: &Dao, search: &SearchClient, ev: &ChangeEvent) -> WorkerResult<()> {
    match (&ev.pk, &ev.sk) {
        (Pk::Team(tid), Sk::Meta) => index_team(dao, search, tid, ev.kind.is_remove()).await,
        // Not an indexable item — nothing to do.
        _ => Ok(()),
    }
}

async fn index_team(
    dao: &Dao,
    search: &SearchClient,
    team_id: &str,
    removed: bool,
) -> WorkerResult<()> {
    if removed {
        search.delete(Index::Teams, team_id).await?;
        return Ok(());
    }
    match dao.get_team_meta(team_id).await? {
        // A tournament's squad team is never searchable: it only exists
        // inside its tournament (see `TeamRecord::tournament_id`).
        Some(t) if t.is_tournament_team() => search.delete(Index::Teams, team_id).await?,
        Some(t) => search.upsert(Index::Teams, &team_doc(&t)).await?,
        None => search.delete(Index::Teams, team_id).await?,
    }
    Ok(())
}

fn team_doc(t: &TeamRecord) -> TeamDoc {
    TeamDoc {
        id: t.id.clone(),
        name: t.name.clone(),
    }
}
