//! Invitation-acceptance linking: flip the roster entry (match player / team
//! member) that carries an invitation from pending → accepted, linking the
//! accepting Agon user when the entry was an unlinked external.
//!
//! Two entry points:
//! - [`Dao::accept_invitation_tx`] — the **synchronous** accept path. In one
//!   `TransactWriteItems` it binds the accepter to the standalone invitation
//!   (status + `invited_user_id` + inbox projection), links the roster entry,
//!   and — for a match invite — writes the accepter's *own* feed row so the game
//!   shows on their feed immediately. Follower fan-out stays async (the stream
//!   event this transaction produces starts the fan-out saga).
//! - [`Dao::link_accepted_invitation`] — the **saga** re-link (external → user)
//!   used by the async accept workflow. Kept for at-least-once replay; it is a
//!   fixed-point re-write of the same accepted state as the transaction above.
//!
//! Idempotent: re-running either against an already-accepted entry re-writes the
//! same accepted state, so the at-least-once accept workflow can replay safely.

use aws_sdk_dynamodb::types::{Put, TransactWriteItem};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, Item};
use super::records::InvitationContextRecord;

impl Dao {
    /// Accept an invitation synchronously and atomically. In one transaction:
    /// 1. rewrite the standalone invitation (`status=accepted`, `responded_at`,
    ///    `invited_user_id=accepting_user_id`, inbox GSI1 projection) — this also
    ///    resolves a bare-token invite to the accepting account;
    /// 2. link the roster entry embedding the invitation to the accepting user
    ///    and mark its embedded invitation accepted;
    /// 3. for a match invite, write the accepter's own feed row so the match is
    ///    on their feed the moment they accept.
    ///
    /// Returns the match id when the invite is to a match (so the caller can
    /// kick off async follower fan-out), or `None` for a team invite.
    ///
    /// `NotFound` if the invitation or its embedding roster entry is gone.
    pub async fn accept_invitation_tx(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
    ) -> DaoResult<Option<String>> {
        let Some(mut inv) = self.get_invitation(invitation_id).await? else {
            return Err(DaoError::NotFound(format!("invitation {invitation_id}")));
        };

        // 1. Bind the accepter onto the standalone invitation. Setting
        //    `invited_user_id` resolves a token invite to the account and gives
        //    the invitation an inbox (`UINV#<uid>`) projection.
        inv.status = "accepted".to_string();
        inv.responded_at = Some(responded_at.to_string());
        inv.invited_user_id = Some(accepting_user_id.to_string());
        let inv_item = self.invitation_item(&inv)?;
        let put_inv = Put::builder()
            .table_name(self.table())
            .set_item(Some(inv_item))
            // Guard on existence so a concurrently-revoked invite fails cleanly.
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // 2 + 3. Link the roster entry, and (match) the accepter's own feed row.
        let (roster_item, feed_put, match_id) = match &inv.context {
            InvitationContextRecord::Match { match_id, .. } => {
                let (item, starts_at) = self
                    .linked_match_player_item(
                        match_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                let feed_item = self.feed_item(accepting_user_id, match_id, &starts_at, now)?;
                let feed_put = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(feed_item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                (item, Some(feed_put), Some(match_id.clone()))
            }
            InvitationContextRecord::Team { team_id, .. } => {
                let item = self
                    .linked_team_member_item(
                        team_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                (item, None, None)
            }
        };

        let put_roster = Put::builder()
            .table_name(self.table())
            .set_item(Some(roster_item))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_inv).build())
            .transact_items(TransactWriteItem::builder().put(put_roster).build());
        if let Some(feed_put) = feed_put {
            tx = tx.transact_items(TransactWriteItem::builder().put(feed_put).build());
        }

        match tx.send().await {
            Ok(_) => Ok(match_id),
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("invitation {invitation_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Build the linked match-player item (external → user) for `invitation_id`,
    /// returning it together with the match's `starts_at` (feed sort material).
    /// Keeps the stable `player_id` so score references survive the flip.
    async fn linked_match_player_item(
        &self,
        match_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<(Item, String)> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Err(DaoError::NotFound(format!("match {match_id}")));
        };
        let starts_at = agg.match_.starts_at.clone();

        let Some(mut player) = agg
            .players
            .into_iter()
            .find(|p| p.invitation.as_ref().is_some_and(|i| i.id == invitation_id))
        else {
            return Err(DaoError::NotFound(format!(
                "no player for invitation {invitation_id} in match {match_id}"
            )));
        };

        player.user_id = Some(accepting_user_id.to_string());
        player.display_name = None;
        if let Some(inv) = player.invitation.as_mut() {
            inv.status = "accepted".to_string();
            inv.responded_at = Some(responded_at.to_string());
        }

        Ok((self.match_player_item(match_id, &player)?, starts_at))
    }

    /// Build the linked team-member item (external → user) for `invitation_id`.
    /// Keeps the stable `membership_id`.
    async fn linked_team_member_item(
        &self,
        team_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<Item> {
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

        self.team_member_item(team_id, &member)
    }

    /// Link the roster entry that embeds `invitation_id` to the accepting user
    /// and mark its embedded invitation accepted. `accepting_user_id` is the
    /// Agon user who accepted (used to link an external entry to an account).
    ///
    /// Used by the async accept saga as an idempotent re-link (the synchronous
    /// [`Self::accept_invitation_tx`] has usually already done this); a fixed-
    /// point re-write, so a replay is harmless.
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
                let (item, _) = self
                    .linked_match_player_item(
                        match_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                self.put_item(item).await
            }
            InvitationContextRecord::Team { team_id, .. } => {
                let item = self
                    .linked_team_member_item(
                        team_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                self.put_item(item).await
            }
        }
    }

    /// Put a fully-formed item map (keys already stamped). Used by the saga
    /// re-link, which builds the item via the shared roster item builders.
    async fn put_item(&self, item: Item) -> DaoResult<()> {
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}
