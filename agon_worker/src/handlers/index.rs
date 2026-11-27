//! Inline handler: keep the Meilisearch indexes in sync with the table.
//!
//! Triggered by writes to a user profile or team meta item. On an
//! insert/modify we re-read the current record from DynamoDB and upsert a search
//! document; on a remove we delete the document by id. Re-reading (rather than
//! trusting a stream image) means we always index the latest committed state.
//!
//! The search documents are intentionally minimal — just what the discovery
//! endpoints need to match on and render a row. Full hydration happens from
//! DynamoDB when a result is opened.

use agon_services_core::dao::Dao;
use agon_services_core::dao::keys::{Pk, Sk};
use agon_services_core::dao::records::{TeamRecord, UserRecord};
use serde::Serialize;

use crate::error::WorkerResult;
use crate::event::ChangeEvent;
use agon_services_core::search::{Index, SearchClient};

/// A user search document (index `users`).
#[derive(Debug, Serialize)]
struct UserDoc {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    profile_image_url: Option<String>,
}

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
        (Pk::User(uid), Sk::Profile) => index_user(dao, search, uid, ev.kind.is_remove()).await,
        (Pk::Team(tid), Sk::Meta) => index_team(dao, search, tid, ev.kind.is_remove()).await,
        // Not an indexable item — nothing to do.
        _ => Ok(()),
    }
}

async fn index_user(
    dao: &Dao,
    search: &SearchClient,
    user_id: &str,
    removed: bool,
) -> WorkerResult<()> {
    if removed {
        search.delete(Index::Users, user_id).await?;
        return Ok(());
    }
    match dao.get_user(user_id).await? {
        Some(u) => search.upsert(Index::Users, &user_doc(&u)).await?,
        // Item gone between the stream event and our read → treat as delete.
        None => search.delete(Index::Users, user_id).await?,
    }
    Ok(())
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

fn user_doc(u: &UserRecord) -> UserDoc {
    UserDoc {
        id: u.id.clone(),
        name: u.name.clone(),
        profile_image_url: u.profile_image_url.clone(),
    }
}

fn team_doc(t: &TeamRecord) -> TeamDoc {
    TeamDoc {
        id: t.id.clone(),
        name: t.name.clone(),
    }
}
