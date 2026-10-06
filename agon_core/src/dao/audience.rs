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
/// Only caps `AudienceMember::known_player_ids` — `known_player_count` still
/// tracks the true total so a client can show it even past the cap.
pub const MAX_KNOWN_PLAYERS: usize = 5;

/// One audience member's per-viewer feed data — see the module doc comment.
#[derive(Debug, Clone, Default)]
pub struct AudienceMember {
    /// Capped at `MAX_KNOWN_PLAYERS` — see `known_player_count` for the true,
    /// uncapped total.
    pub known_player_ids: Vec<String>,
    /// How many of the match's participants this viewer follows, in total —
    /// unlike `known_player_ids`, never capped, so a client can render "+N
    /// more" beyond the hydrated list even when more than `MAX_KNOWN_PLAYERS`
    /// participants are followed.
    pub known_player_count: u32,
    /// The side this viewer plays on, if they're themselves a participant —
    /// `None` for a viewer in the audience only via a follow (they're not
    /// playing) or a participant not yet assigned a side. Do NOT use this to
    /// decide "is the viewer going" — an unassigned participant (a match with
    /// `allow_unassigned` whose joiner hasn't picked a side) has this as
    /// `None` too despite playing. Use `viewer_is_going` for that; this field
    /// is only for the side-specific score confirm/dispute prompt.
    pub viewer_side_id: Option<String>,
    /// Whether this viewer is themselves a participant in the match (in the
    /// `agg.players` loop below), regardless of whether they've been
    /// assigned a side yet. Unlike `viewer_side_id`, this is `true` for an
    /// unassigned participant, so it's the right field for "is the viewer
    /// going" (e.g. a feed card's RSVP state).
    pub viewer_is_going: bool,
    /// Whether this viewer has a *pending* (not yet responded to) invitation
    /// to the match — distinct from `viewer_is_going`, which is `false` for
    /// a pending invitee. Lets "Coming up" surface a match the viewer hasn't
    /// accepted yet, rather than only matches they're already in.
    pub viewer_invitation_pending: bool,
    /// Side ids the viewer may join directly via an accepted membership on
    /// that side's team (`MatchSideRecord::team_join_enabled`) — see
    /// `Self::collect_team_join_eligible_members`. Not filtered by whether
    /// the viewer is already a participant (harmless either way: a client
    /// checks `viewer_is_going` first).
    pub viewer_can_join_side_ids: Vec<String>,
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
                // players" list — you're never your own follower). Only a
                // player who actually occupies a roster spot (no invitation,
                // or an accepted one) is "going" — a pending invitee isn't,
                // even though they already have a `user_id`.
                let entry = audience.entry(user_id.clone()).or_default();
                entry.viewer_side_id = player.side_id.clone();
                entry.viewer_is_going = player.occupies_slot();
                entry.viewer_invitation_pending = player
                    .invitation
                    .as_ref()
                    .is_some_and(|inv| inv.status == "pending");
                self.collect_user_followers(user_id, &mut audience).await?;
            }
        }

        // Involved teams (sides with a team) → their followers. No specific
        // player to attribute, so they just join the audience.
        for side in &agg.sides {
            if let Some(team_id) = &side.team_id {
                self.collect_team_followers(team_id, &mut audience).await?;
                // A side open to team self-join additionally earns every
                // accepted team member a `viewer_can_join_side_ids` entry
                // (see that method's doc comment) — distinct from merely
                // following the team, which every member already does too
                // (see `Dao::follow_team`'s doc comment), but following
                // alone doesn't tell a client "you specifically can join
                // this side".
                if side.team_join_enabled {
                    self.collect_team_join_eligible_members(
                        team_id,
                        &side.side_id,
                        &mut audience,
                    )
                    .await?;
                }
            }
        }

        Ok(audience)
    }

    /// Walk a participating user's followers, adding each to `audience` (as
    /// an audience member) and recording that they follow `user_id`: always
    /// in `known_player_count`, and — capped at `MAX_KNOWN_PLAYERS` — in
    /// `known_player_ids`.
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
                if entry.known_player_ids.iter().any(|id| id == user_id) {
                    continue;
                }
                entry.known_player_count += 1;
                if entry.known_player_ids.len() < MAX_KNOWN_PLAYERS {
                    entry.known_player_ids.push(user_id.to_string());
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

    /// Give every accepted member of `team_id` a `viewer_can_join_side_ids`
    /// entry for `side_id` — mirrors `caller_team_join_sides`'
    /// (`agon_service`) single-caller eligibility check, but for the whole
    /// team at once, since fan-out already needs to visit every member. One
    /// `get_team` covers every member in one query, same cost class as
    /// `collect_team_followers`'s own per-team page walk.
    async fn collect_team_join_eligible_members(
        &self,
        team_id: &str,
        side_id: &str,
        audience: &mut HashMap<String, AudienceMember>,
    ) -> DaoResult<()> {
        let Some(agg) = self.get_team(team_id).await? else {
            return Ok(());
        };
        for member in &agg.members {
            let Some(user_id) = &member.user_id else {
                continue;
            };
            let accepted = member
                .invitation
                .as_ref()
                .is_none_or(|inv| inv.status == "accepted");
            if !accepted {
                continue;
            }
            let entry = audience.entry(user_id.clone()).or_default();
            entry.viewer_can_join_side_ids.push(side_id.to_string());
        }
        Ok(())
    }
}
