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
    /// ISO-8601 start time, for display / hydration.
    starts_at: String,
    /// Start time as a Unix timestamp (seconds). Meilisearch range filters
    /// (`>=` / `<=`) and sorts are numeric-only — they can't compare ISO
    /// strings — so date filtering and ordering use this numeric field.
    starts_at_ts: i64,
    /// Ids that identify a participant of this match — both linked user ids and
    /// the stable player ids — so the `participant` discovery filter matches
    /// either. Deduplicated.
    participant_ids: Vec<String>,
    /// Which of `participant_ids` won / lost / drew, split into three
    /// buckets rather than a single per-participant map — Meilisearch
    /// documents are per-match, not per-viewer, so there's no single
    /// "outcome" field to fetch; a search already filtered to one
    /// `participant_ids` value can instead check which bucket that id
    /// landed in, for free, off the same hit (see `SearchClient::search_matches`).
    /// All three are empty until the match has a confirmed score — a
    /// missing outcome, not a draw.
    winning_participant_ids: Vec<String>,
    losing_participant_ids: Vec<String>,
    /// Populated only when the confirmed score has no single winner (a tie) —
    /// every participant with an assigned side lands here.
    drawing_participant_ids: Vec<String>,
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
/// Temporal `index_match` activity can reuse the exact same doc shape.
pub fn match_search_doc(agg: &MatchAggregate) -> MatchDoc {
    match_doc(agg)
}

fn match_doc(agg: &MatchAggregate) -> MatchDoc {
    let m = &agg.match_;
    // Both linked user ids and stable player ids identify a participant, so the
    // discovery `participant` filter matches whichever the caller supplies.
    let mut participant_ids = std::collections::BTreeSet::new();
    let mut winning_participant_ids = std::collections::BTreeSet::new();
    let mut losing_participant_ids = std::collections::BTreeSet::new();
    let mut drawing_participant_ids = std::collections::BTreeSet::new();
    // `None` = no confirmed result yet, so every player's ids stay out of all
    // three outcome buckets. `Some(None)` = confirmed but tied (no single
    // winning side). `Some(Some(side))` = that side won.
    let winner_side_id: Option<Option<&String>> = m
        .confirmed_score
        .as_ref()
        .map(|cs| cs.winner_side_id.as_ref());
    for player in &agg.players {
        let mut ids = vec![player.player_id.clone()];
        if let Some(uid) = &player.user_id {
            ids.push(uid.clone());
        }
        participant_ids.extend(ids.iter().cloned());
        // A player who was never assigned a side (shouldn't happen for a
        // finished match, but no roster invariant guarantees it) has no
        // outcome to report — leave them out of every bucket.
        if let (Some(side_id), Some(winner)) = (&player.side_id, winner_side_id) {
            let bucket = match winner {
                Some(winning_side) if winning_side == side_id => &mut winning_participant_ids,
                Some(_) => &mut losing_participant_ids,
                None => &mut drawing_participant_ids,
            };
            bucket.extend(ids);
        }
    }
    // Parse the ISO-8601 start time to a Unix timestamp for numeric filter/sort.
    // A malformed/absent timestamp falls back to 0 (epoch) rather than dropping
    // the document — the match is still discoverable by text/sport, just sorts
    // oldest.
    let starts_at_ts = chrono::DateTime::parse_from_rfc3339(&m.starts_at)
        .map(|dt| dt.timestamp())
        .unwrap_or(0);
    MatchDoc {
        id: m.id.clone(),
        name: m.name.clone(),
        sport: m.match_type.clone(),
        status: m.status.clone(),
        starts_at: m.starts_at.clone(),
        starts_at_ts,
        participant_ids: participant_ids.into_iter().collect(),
        winning_participant_ids: winning_participant_ids.into_iter().collect(),
        losing_participant_ids: losing_participant_ids.into_iter().collect(),
        drawing_participant_ids: drawing_participant_ids.into_iter().collect(),
    }
}
