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

use aws_sdk_dynamodb::types::{Delete, Put, TransactWriteItem};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, Item, s};
use super::keys::{Pk, Sk};
use super::match_ops::take_slot_update;
use super::records::{InvitationContextRecord, MatchPlayerRecord};

impl Dao {
    /// Accept an invitation synchronously and atomically, into a real roster
    /// spot (never onto the waitlist — see `Dao::accept_invitation_onto_waitlist_tx`
    /// for that). In one transaction:
    /// 1. rewrite the standalone invitation (`status=accepted`, `responded_at`,
    ///    `invited_user_id=accepting_user_id`, inbox GSI1 projection) — this also
    ///    resolves a bare-token invite to the accepting account;
    /// 2. link the roster entry embedding the invitation to the accepting user
    ///    and mark its embedded invitation accepted;
    /// 3. for a match invite, write the accepter's own feed row, and — the
    ///    cap-guarded last-moment race check, same split as `Dao::join_match_tx`
    ///    — bump `total_player_count`/the target side's `player_count`.
    ///
    /// Returns the match id when the invite is to a match (so the caller can
    /// kick off async follower fan-out), or `None` for a team invite.
    ///
    /// `NotFound` if the invitation is gone at the initial read. `Conflict` if
    /// the transaction's own conditions fail instead — the invitation was
    /// revoked concurrently, *or* (match invite only) the cap guard tripped.
    /// The two aren't distinguished here; a caller that needs to (the
    /// `respond_to_invitation` handlers, to return the right typed error)
    /// re-reads to classify which, the same way `join_match`'s own handler
    /// re-classifies its atomic guard's failure.
    #[tracing::instrument(skip(self))]
    pub async fn accept_invitation_tx(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
    ) -> DaoResult<Option<String>> {
        let (match_id, items) = self
            .accept_invitation_items(invitation_id, accepting_user_id, responded_at, now, false)
            .await?;
        self.send_transact_items(items).await?;
        Ok(match_id)
    }

    /// Build the writes `accept_invitation_tx` sends as one transaction —
    /// invitation, roster entry, (match) the accepter's own feed row, plus
    /// (match, real spot only) the cap-guarded headcount `ADD` — without
    /// sending them. Shared with `Dao::accept_invitation_onto_waitlist_tx`,
    /// which appends one more item (the waitlist entry put) to the same list
    /// before sending.
    ///
    /// `onto_waitlist` changes what "link the roster entry" means for a match
    /// invite: `false` (an ordinary accept) links it as a real, counted
    /// roster row, cap-guarded. `true` *deletes* the pending placeholder row
    /// instead — the accepter isn't actually in yet, so there's nothing to
    /// link and no cap to guard (the waitlist is uncapped); the separate
    /// `MatchWaitlistEntryRecord` `accept_invitation_onto_waitlist_tx` adds is
    /// the only record of them from here on, until a match admin moves them
    /// in. Ignored for a team invite — teams have no waitlist.
    ///
    /// Returns the match id alongside (`None` for a team invite) since the
    /// caller needs it before the transaction is sent (to build the waitlist
    /// item), not just after.
    pub(super) async fn accept_invitation_items(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
        onto_waitlist: bool,
    ) -> DaoResult<(Option<String>, Vec<TransactWriteItem>)> {
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

        // 2 + 3. Link (or, onto the waitlist, remove) the roster entry, and
        // (match) the accepter's own feed row + cap-guarded headcount `ADD`.
        let (roster_write, feed_put, match_id, slot_update) = match &inv.context {
            InvitationContextRecord::Match { match_id, .. } => {
                let (existing, starts_at, side_max_players, total_max_players) = self
                    .match_player_for_invitation(match_id, invitation_id)
                    .await?;

                let feed_item = self.feed_item(
                    accepting_user_id,
                    match_id,
                    &starts_at,
                    now,
                    &AudienceMember {
                        viewer_side_id: existing.side_id.clone(),
                        ..Default::default()
                    },
                )?;
                let feed_put = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(feed_item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;

                if onto_waitlist {
                    // Not actually in — delete the pending placeholder rather
                    // than link it. No headcount change: it didn't count
                    // while pending, and it still won't while waitlisted.
                    let delete = Delete::builder()
                        .table_name(self.table())
                        .key(ATTR_PK, s(Pk::Match(match_id.clone()).to_string()))
                        .key(
                            ATTR_SK,
                            s(Sk::Player(existing.player_id.clone()).to_string()),
                        )
                        .build()
                        .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                    (
                        TransactWriteItem::builder().delete(delete).build(),
                        Some(feed_put),
                        Some(match_id.clone()),
                        None,
                    )
                } else {
                    let mut linked = existing;
                    linked.user_id = Some(accepting_user_id.to_string());
                    linked.display_name = None;
                    if let Some(inv) = linked.invitation.as_mut() {
                        inv.status = "accepted".to_string();
                        inv.responded_at = Some(responded_at.to_string());
                    }
                    let put_roster = Put::builder()
                        .table_name(self.table())
                        .set_item(Some(self.match_player_item(match_id, &linked)?))
                        .build()
                        .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                    let slot_update = take_slot_update(
                        self.table(),
                        match_id,
                        linked.side_id.as_deref(),
                        side_max_players,
                        total_max_players,
                    )?;
                    (
                        TransactWriteItem::builder().put(put_roster).build(),
                        Some(feed_put),
                        Some(match_id.clone()),
                        Some(slot_update),
                    )
                }
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
                let put_roster = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                (
                    TransactWriteItem::builder().put(put_roster).build(),
                    None,
                    None,
                    None,
                )
            }
        };

        let mut items = vec![
            TransactWriteItem::builder().put(put_inv).build(),
            roster_write,
        ];
        if let Some(feed_put) = feed_put {
            items.push(TransactWriteItem::builder().put(feed_put).build());
        }
        if let Some(slot_update) = slot_update {
            items.push(TransactWriteItem::builder().update(slot_update).build());
        }
        Ok((match_id, items))
    }

    /// Send a prebuilt list of writes as one `TransactWriteItems` call.
    /// `Conflict` on any failed item condition — a caller that needs to know
    /// *which* one re-reads and reclassifies rather than this inspecting the
    /// transaction's cancellation reasons (the same choice `join_match`'s own
    /// handler makes for its atomic guard's failure).
    pub(super) async fn send_transact_items(&self, items: Vec<TransactWriteItem>) -> DaoResult<()> {
        let mut tx = self.client.transact_write_items();
        for item in items {
            tx = tx.transact_items(item);
        }
        match tx.send().await {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Err(DaoError::Conflict(
                "one or more of this transaction's conditions were not met".into(),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Find the roster row embedding `invitation_id`, unmodified, together
    /// with match context needed either to link it (a real accept — see
    /// `accept_invitation_items`, which does the actual mutating) or to know
    /// what's being removed (an accept onto the waitlist): the match's
    /// `starts_at` (feed sort material), the target side's own cap, and the
    /// match's derived overall cap (`take_slot_update`'s guard — `None` for
    /// either when there's no side/no cap).
    async fn match_player_for_invitation(
        &self,
        match_id: &str,
        invitation_id: &str,
    ) -> DaoResult<(MatchPlayerRecord, String, Option<u32>, Option<u32>)> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Err(DaoError::NotFound(format!("match {match_id}")));
        };
        let total_max_players = agg.effective_max_players();

        let Some(player) = agg
            .players
            .into_iter()
            .find(|p| p.invitation.as_ref().is_some_and(|i| i.id == invitation_id))
        else {
            return Err(DaoError::NotFound(format!(
                "no player for invitation {invitation_id} in match {match_id}"
            )));
        };
        let side_max_players = player
            .side_id
            .as_deref()
            .and_then(|sid| agg.sides.iter().find(|s| s.side_id == sid))
            .and_then(|s| s.max_players);

        Ok((
            player,
            agg.match_.starts_at,
            side_max_players,
            total_max_players,
        ))
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
    /// point re-write, so a replay is harmless. The saga fires off the
    /// invitation's own `pending` → `accepted` stream transition, so it runs
    /// the same regardless of whether the sync accept landed in a real spot or
    /// onto the waitlist (`Dao::accept_invitation_onto_waitlist_tx`) — for the
    /// latter, the sync transaction already *deleted* the roster row and
    /// created a `MatchWaitlistEntryRecord` instead, so the fixed point this
    /// replay converges on is "no roster row, a live waitlist entry" rather
    /// than a linked one. Checking for that entry first and no-op'ing is what
    /// makes this still idempotent in that case rather than failing `NotFound`
    /// against a row that was deleted on purpose.
    ///
    /// Returns `NotFound` if the invitation or (a real accept only) its target
    /// entry is gone.
    #[tracing::instrument(skip(self))]
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
                if self
                    .get_waitlist_entry(match_id, accepting_user_id)
                    .await?
                    .is_some()
                {
                    return Ok(());
                }
                let (mut player, _, _, _) = self
                    .match_player_for_invitation(match_id, invitation_id)
                    .await?;
                player.user_id = Some(accepting_user_id.to_string());
                player.display_name = None;
                if let Some(inv) = player.invitation.as_mut() {
                    inv.status = "accepted".to_string();
                    inv.responded_at = Some(responded_at.to_string());
                }
                let item = self.match_player_item(match_id, &player)?;
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
