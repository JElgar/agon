//! Invitation-acceptance linking: flip the roster entry (match player / team
//! member) that carries an invitation from pending → accepted, linking the
//! accepting Agon user when the entry was an unlinked external.
//!
//! The standalone `InvitationRecord` status is flipped separately by
//! `respond_to_invitation` (in the API handler). This module handles the
//! *roster* side of the saga: finding the player/member that embeds the
//! invitation and updating it. Keyed off the invitation's `context`, so the
//! caller only needs the invitation record.
//!
//! Idempotent: re-running against an already-accepted entry re-writes the same
//! accepted state, so the at-least-once accept workflow can replay safely.

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::records::InvitationContextRecord;

impl Dao {
    /// Link the roster entry that embeds `invitation_id` to the accepting user
    /// and mark its embedded invitation accepted. `accepting_user_id` is the
    /// Agon user who accepted (used to link an external entry to an account).
    ///
    /// Returns `NotFound` if the invitation or its target entry is gone.
    pub async fn link_accepted_invitation(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<()> {
        let Some(inv) = self.get_invitation(invitation_id).await? else {
            return Err(DaoError::NotFound(format!("invitation {invitation_id}")));
        };

        match &inv.context {
            InvitationContextRecord::Match { match_id, .. } => {
                self.link_match_player(match_id, invitation_id, accepting_user_id, responded_at)
                    .await
            }
            InvitationContextRecord::Team { team_id, .. } => {
                self.link_team_member(team_id, invitation_id, accepting_user_id, responded_at)
                    .await
            }
        }
    }

    /// Find the match player embedding `invitation_id`, link it to the accepting
    /// user, and mark its embedded invitation accepted. Keeps the stable
    /// `player_id` so score references survive the External → User flip.
    async fn link_match_player(
        &self,
        match_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<()> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Err(DaoError::NotFound(format!("match {match_id}")));
        };

        let Some(mut player) = agg
            .players
            .into_iter()
            .find(|p| p.invitation.as_ref().is_some_and(|i| i.id == invitation_id))
        else {
            return Err(DaoError::NotFound(format!(
                "no player for invitation {invitation_id} in match {match_id}"
            )));
        };

        // Link the account (a no-op if already linked) and clear the external
        // display name now that we have a real user.
        player.user_id = Some(accepting_user_id.to_string());
        player.display_name = None;
        if let Some(inv) = player.invitation.as_mut() {
            inv.status = "accepted".to_string();
            inv.responded_at = Some(responded_at.to_string());
        }

        self.put_match_player(match_id, &player).await
    }

    /// Find the team member embedding `invitation_id`, link it to the accepting
    /// user, and mark its embedded invitation accepted. Keeps the stable
    /// `membership_id`.
    async fn link_team_member(
        &self,
        team_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<()> {
        let Some(agg) = self.get_team(team_id).await? else {
            return Err(DaoError::NotFound(format!("team {team_id}")));
        };

        let Some(mut member) = agg
            .members
            .into_iter()
            .find(|m| m.invitation.as_ref().is_some_and(|i| i.id == invitation_id))
        else {
            return Err(DaoError::NotFound(format!(
                "no member for invitation {invitation_id} in team {team_id}"
            )));
        };

        member.user_id = Some(accepting_user_id.to_string());
        member.display_name = None;
        if let Some(inv) = member.invitation.as_mut() {
            inv.status = "accepted".to_string();
            inv.responded_at = Some(responded_at.to_string());
        }

        self.put_team_member(team_id, &member).await
    }
}
