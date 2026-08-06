//! Fan-out audience resolution: given a match, compute the deduplicated set of
//! viewer ids whose feed should receive it, plus — for each viewer — the
//! per-viewer feed data that lets their feed card render without a live
//! players query: which of the match's participants they follow ("people you
//! know are playing"), and, if the viewer is themselves a participant, which
//! side they play on (so their own card can show the score confirm/dispute
//! prompt).
//!
//! Per docs/async-design.md §5/§11 the audience is the union of:
//! - followers of every **participating user** (players with a linked user id),
//! - followers of every **involved team** (sides with a team id),
//! - the **participants themselves** (so a user's own matches appear in their
//!   own feed).
//!
//! Deduplicated across all three. Feed writes are idempotent on the match id
//! anyway, so an accidental duplicate is harmless — dedup just avoids wasted
//! writes.
//!
//! Both per-viewer fields fall out of the same walk that builds the audience:
//! resolving a participant's followers already visits, for each follower, the
//! exact fact "this viewer follows this participant" — it used to be
//! discarded once the id was inserted into the audience set. Capturing it
//! instead costs nothing extra (same `list_user_followers` pages). Neither
//! field is live: both are a snapshot as of the last time this resolved for
//! the match (creation, an invitation acceptance, or a meta update
//! re-running fan-out), refreshed via `write_feed_items`' full-item overwrite
//! — see each field's doc comment on `FeedItemRecord` for what triggers that.

use std::collections::HashMap;

use super::client::Dao;
use super::error::DaoResult;

/// How many follower rows to pull per page while walking a follower list.
const FOLLOWER_PAGE: u32 = 100;

/// Cap on how many followed participants are recorded per viewer, per match
/// ("people you know are playing"). Keeps feed items small and, combined with
/// the feed endpoint's own page-size cap, bounds the read-time
/// `batch_get_users` hydration of known players to a single `BatchGetItem`.
pub const MAX_KNOWN_PLAYERS: usize = 5;

/// One audience member's per-viewer feed data — see the module doc comment.
#[derive(Debug, Clone, Default)]
pub struct AudienceMember {
    pub known_player_ids: Vec<String>,
    /// The side this viewer plays on, if they're themselves a participant —
    /// `None` for a viewer in the audience only via a follow (they're not
    /// playing) or a participant not yet assigned a side.
    pub viewer_side_id: Option<String>,
}

impl Dao {
    /// Resolve a match's fan-out audience, keyed by viewer id, with each
    /// viewer's per-viewer feed data (see [`AudienceMember`]). Returns an
    /// empty map if the match doesn't exist.
    #[tracing::instrument(skip(self))]
    pub async fn resolve_fanout_audience(
        &self,
        match_id: &str,
    ) -> DaoResult<HashMap<String, AudienceMember>> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Ok(HashMap::new());
        };

        let mut audience: HashMap<String, AudienceMember> = HashMap::new();

        // Participating users (players linked to an account) + their followers.
        for player in &agg.players {
            if let Some(user_id) = &player.user_id {
                // The participant's own entry: record which side they play
                // on (their card shows the score prompt, not a "known
                // players" list — you're never your own follower).
                let entry = audience.entry(user_id.clone()).or_default();
                entry.viewer_side_id = player.side_id.clone();
                self.collect_user_followers(user_id, &mut audience).await?;
            }
        }

        // Involved teams (sides with a team) → their followers. No specific
        // player to attribute, so they just join the audience.
        for side in &agg.sides {
            if let Some(team_id) = &side.team_id {
                self.collect_team_followers(team_id, &mut audience).await?;
            }
        }

        Ok(audience)
    }

    /// Walk a participating user's followers, adding each to `audience` (as
    /// an audience member) and, capped at `MAX_KNOWN_PLAYERS`, recording that
    /// they follow `user_id`.
    async fn collect_user_followers(
        &self,
        user_id: &str,
        audience: &mut HashMap<String, AudienceMember>,
    ) -> DaoResult<()> {
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_user_followers(user_id, cursor.as_deref(), FOLLOWER_PAGE)
                .await?;
            for edge in &page.items {
                let entry = audience.entry(edge.follower_id.clone()).or_default();
                let known = &mut entry.known_player_ids;
                if known.len() < MAX_KNOWN_PLAYERS && !known.iter().any(|id| id == user_id) {
                    known.push(user_id.to_string());
                }
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(())
    }

    /// Walk a team's followers, adding each to `audience` as an audience
    /// member (no specific participant to attribute the follow to).
    async fn collect_team_followers(
        &self,
        team_id: &str,
        audience: &mut HashMap<String, AudienceMember>,
    ) -> DaoResult<()> {
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_team_followers(team_id, cursor.as_deref(), FOLLOWER_PAGE)
                .await?;
            for edge in &page.items {
                audience.entry(edge.follower_id.clone()).or_default();
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(())
    }
}
