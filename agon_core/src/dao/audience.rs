//! Fan-out audience resolution: given a match, compute the deduplicated set of
//! viewer ids whose feed should receive it.
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

use std::collections::BTreeSet;

use super::client::Dao;
use super::error::DaoResult;

/// How many follower rows to pull per page while walking a follower list.
const FOLLOWER_PAGE: u32 = 100;

impl Dao {
    /// Resolve the deduplicated set of viewer ids for a match's fan-out.
    ///
    /// Reads the match aggregate, then walks the follower lists of each
    /// participating user and involved team. Returns an empty set if the match
    /// doesn't exist.
    pub async fn resolve_fanout_audience(&self, match_id: &str) -> DaoResult<Vec<String>> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Ok(Vec::new());
        };

        let mut audience: BTreeSet<String> = BTreeSet::new();

        // Participating users (players linked to an account) + their followers.
        for player in &agg.players {
            if let Some(user_id) = &player.user_id {
                audience.insert(user_id.clone()); // the participant themselves
                self.collect_user_followers(user_id, &mut audience).await?;
            }
        }

        // Involved teams (sides with a team) → their followers.
        for side in &agg.sides {
            if let Some(team_id) = &side.team_id {
                self.collect_team_followers(team_id, &mut audience).await?;
            }
        }

        Ok(audience.into_iter().collect())
    }

    /// Add every follower of a user into `out`, paging through the follower list.
    async fn collect_user_followers(
        &self,
        user_id: &str,
        out: &mut BTreeSet<String>,
    ) -> DaoResult<()> {
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_user_followers(user_id, cursor.as_deref(), FOLLOWER_PAGE)
                .await?;
            for edge in &page.items {
                out.insert(edge.follower_id.clone());
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(())
    }

    /// Add every follower of a team into `out`, paging through the follower list.
    async fn collect_team_followers(
        &self,
        team_id: &str,
        out: &mut BTreeSet<String>,
    ) -> DaoResult<()> {
        let mut cursor: Option<String> = None;
        loop {
            let page = self
                .list_team_followers(team_id, cursor.as_deref(), FOLLOWER_PAGE)
                .await?;
            for edge in &page.items {
                out.insert(edge.follower_id.clone());
            }
            match page.next_cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        Ok(())
    }
}
