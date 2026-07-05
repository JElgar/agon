//! Inline handler: keep the Meilisearch indexes in sync with the table.
//!
//! Triggered by writes to a user profile, team meta, or match meta item. On an
//! insert/modify we re-read the current record from DynamoDB and upsert a search
//! document; on a remove we delete the document by id. Re-reading (rather than
//! trusting a stream image) means we always index the latest committed state.
//!
//! The search documents are intentionally minimal — just what the discovery
//! endpoints need to match on and render a row. Full hydration happens from
//! DynamoDB when a result is opened.

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::match_ops::MatchAggregate;
use agon_core::dao::records::{TeamRecord, UserRecord};
use serde::Serialize;

use crate::error::WorkerResult;
use crate::event::ChangeEvent;
use agon_core::search::{Index, SearchClient};

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

/// A match search document (index `matches`). Carries the fields the discovery
/// endpoint filters and sorts on.
#[derive(Debug, Serialize)]
pub struct MatchDoc {
    id: String,
    name: String,
    sport: String,
    status: String,
    starts_at: String,
    /// Ids that identify a participant of this match — both linked user ids and
    /// the stable player ids — so the `participant` discovery filter matches
    /// either. Deduplicated.
    participant_ids: Vec<String>,
}

/// Handle an index-relevant change event. Returns `Ok(())` for events that are
/// not index-relevant (they're simply ignored — the router only routes the
/// relevant ones, but this stays total for safety).
pub async fn handle(dao: &Dao, search: &SearchClient, ev: &ChangeEvent) -> WorkerResult<()> {
    match (&ev.pk, &ev.sk) {
        (Pk::User(uid), Sk::Profile) => index_user(dao, search, uid, ev.kind.is_remove()).await,
        (Pk::Team(tid), Sk::Meta) => index_team(dao, search, tid, ev.kind.is_remove()).await,
        (Pk::Match(mid), Sk::Meta) => index_match(dao, search, mid, ev.kind.is_remove()).await,
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
        Some(t) => search.upsert(Index::Teams, &team_doc(&t)).await?,
        None => search.delete(Index::Teams, team_id).await?,
    }
    Ok(())
}

async fn index_match(
    dao: &Dao,
    search: &SearchClient,
    match_id: &str,
    removed: bool,
) -> WorkerResult<()> {
    if removed {
        search.delete(Index::Matches, match_id).await?;
        return Ok(());
    }
    match dao.get_match(match_id).await? {
        Some(agg) => search.upsert(Index::Matches, &match_doc(&agg)).await?,
        None => search.delete(Index::Matches, match_id).await?,
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

/// Build the match search document from a match aggregate. Public so the
/// Temporal `index_match` activity can reuse the exact same doc shape. Only the
/// `temporal` feature calls this, so it's gated to keep the default build clean.
#[cfg(feature = "temporal")]
pub fn match_search_doc(agg: &MatchAggregate) -> MatchDoc {
    match_doc(agg)
}

fn match_doc(agg: &MatchAggregate) -> MatchDoc {
    let m = &agg.match_;
    // Both linked user ids and stable player ids identify a participant, so the
    // discovery `participant` filter matches whichever the caller supplies.
    let mut participant_ids = std::collections::BTreeSet::new();
    for player in &agg.players {
        participant_ids.insert(player.player_id.clone());
        if let Some(uid) = &player.user_id {
            participant_ids.insert(uid.clone());
        }
    }
    MatchDoc {
        id: m.id.clone(),
        name: m.name.clone(),
        sport: m.match_type.clone(),
        status: m.status.clone(),
        starts_at: m.starts_at.clone(),
        participant_ids: participant_ids.into_iter().collect(),
    }
}
