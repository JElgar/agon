//! Invitation-acceptance linking: flip the roster entry (match player / team
//! member) that carries an invitation from pending → accepted, linking the
//! accepting Agon user when the entry was an unlinked external.
//!
//! Two entry points:
//! - [`Dao::accept_invitation_tx`] — the **synchronous** accept path. In one
//!   `TransactWriteItems` it binds the accepter to the standalone invitation
//!   (status + `invited_user_id` + inbox projection), links the roster entry,
//!   and — for a match invite — writes the accepter's *own* feed row so the game
//!   shows on their feed immediately, and takes their spot in the match's
//!   headcounts. Follower fan-out stays async (the stream event this
//!   transaction produces starts the fan-out saga).
//! - [`Dao::link_accepted_invitation`] — the **saga** re-link (external → user)
//!   used by the async accept workflow. Kept for at-least-once replay; it is a
//!   fixed-point re-write of the same accepted state as the transaction above,
//!   and never touches the headcounts.
//!
//! Idempotent: re-running either against an already-accepted entry re-writes the
//! same accepted state — taking no second spot — so the at-least-once accept
//! workflow (or a repeated accept request) can replay safely.

use aws_sdk_dynamodb::types::{AttributeValue, Put, TransactWriteItem, Update};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, Item, s};
use super::keys::{Pk, Sk};
use super::records::InvitationContextRecord;

/// How many times [`Dao::accept_invitation_tx`] re-reads and tries again after
/// the transaction that would take a spot is cancelled by a condition (see
/// that method). One retry normally settles it — the re-read finds the row
/// already accepted and re-writes without counting — so the bound only stops
/// a row that keeps changing under us from looping.
const ACCEPT_ATTEMPTS: u32 = 3;

/// The outcome of one [`Dao::try_accept_invitation`] attempt.
enum AcceptAttempt {
    /// Committed. Carries the match id for a match invite.
    Accepted(Option<String>),
    /// The attempt would have taken a spot, and a condition cancelled it —
    /// most likely a concurrent accept of the same invite got there first.
    /// Nothing was written; re-read and try again.
    SlotGuardFailed,
}

/// A match roster row rebuilt as linked and accepted, plus what the accept
/// paths need from the same read. See [`Dao::linked_match_player_item`].
struct LinkedMatchPlayer {
    item: Item,
    /// The match's `starts_at` — feed sort material.
    starts_at: String,
    /// The player's side — feed material too (the accepter's own feed row
    /// records which side they play on).
    side_id: Option<String>,
    /// Whether the row *as read* already took a spot
    /// (`MatchPlayerRecord::occupies_slot`) — i.e. this accept is a re-write
    /// of an accepted invite, not the transition that takes one.
    already_occupies_slot: bool,
    /// `side_id`, but only if the match still has that side — the side whose
    /// `player_count` an accept may `ADD` to. A roster row can carry a side id
    /// the match doesn't have (`update_match`'s `side_assignments` doesn't
    /// validate it), and an `ADD` to a missing `sides.<id>` is an invalid
    /// document path that would cancel the whole accept.
    counted_side_id: Option<String>,
}

impl Dao {
    /// Accept an invitation synchronously and atomically. In one transaction:
    /// 1. rewrite the standalone invitation (`status=accepted`, `responded_at`,
    ///    `invited_user_id=accepting_user_id`, inbox GSI1 projection) — this also
    ///    resolves a bare-token invite to the accepting account;
    /// 2. link the roster entry embedding the invitation to the accepting user
    ///    and mark its embedded invitation accepted;
    /// 3. for a match invite, write the accepter's own feed row so the match is
    ///    on their feed the moment they accept;
    /// 4. for a match invite not already accepted, take the accepter's spot:
    ///    `ADD` 1 to the match's `total_player_count`, and to their side's
    ///    `player_count` if they're on one.
    ///
    /// The spot is counted here, in the accept itself, rather than left to the
    /// accept saga's later recount (`refresh_side_roster_previews`), so the
    /// counts are right the moment the accept returns — a joiner right behind
    /// it can't be let into the spot it just took. It's an atomic `ADD` for the
    /// same reason `join_match_tx`'s is: a count computed from a read and
    /// written back would race.
    ///
    /// **Counted once.** Accepting an already-accepted invite (a double tap, a
    /// retried request) stays the idempotent re-write it always was and takes
    /// no second spot: step 4 is only included when the roster row as read
    /// isn't accepted, and the roster write is then conditioned on it *still*
    /// not being accepted. If a concurrent accept of the same invite commits
    /// in between, this transaction is cancelled whole — no `ADD` — and
    /// retried from a fresh read, which finds the row accepted and re-writes
    /// it without counting. The guard says "not accepted" rather than
    /// "pending" because it's the negation of `MatchPlayerRecord::occupies_slot`:
    /// what's being counted is the transition from holding no spot to holding
    /// one, whatever the status before. (As a side effect it also stops a
    /// counting accept re-creating a roster row removed since the read.)
    ///
    /// **No capacity check.** Accepting an invite has never been turned away
    /// for a full match and still isn't, so an accept can take a match over
    /// its cap. Deliberate until there's somewhere else to put the accepter (a
    /// waitlist) — rejecting them outright would be a regression.
    ///
    /// Returns the match id when the invite is to a match (so the caller can
    /// kick off async follower fan-out), or `None` for a team invite.
    ///
    /// `NotFound` if the invitation or its embedding roster entry is gone.
    #[tracing::instrument(skip(self))]
    pub async fn accept_invitation_tx(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
    ) -> DaoResult<Option<String>> {
        for attempt in 0..ACCEPT_ATTEMPTS {
            super::batch::backoff(attempt).await;
            match self
                .try_accept_invitation(invitation_id, accepting_user_id, responded_at, now)
                .await?
            {
                AcceptAttempt::Accepted(match_id) => return Ok(match_id),
                AcceptAttempt::SlotGuardFailed => continue,
            }
        }
        // Cancelled every time: the roster row or invitation keeps changing
        // under us, or has gone and our re-reads haven't caught up. Report it
        // the way a revoked invite is reported.
        Err(DaoError::NotFound(format!("invitation {invitation_id}")))
    }

    /// One read-then-write attempt at [`Self::accept_invitation_tx`].
    async fn try_accept_invitation(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
    ) -> DaoResult<AcceptAttempt> {
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

        // 2 + 3 (+ 4). Link the roster entry, and (match) the accepter's own
        //    feed row and — unless they'd already accepted — their spot.
        let (roster_item, feed_put, match_id, take_slot) = match &inv.context {
            InvitationContextRecord::Match { match_id, .. } => {
                let linked = self
                    .linked_match_player_item(
                        match_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                let feed_item = self.feed_item(
                    accepting_user_id,
                    match_id,
                    &linked.starts_at,
                    now,
                    &AudienceMember {
                        viewer_side_id: linked.side_id,
                        ..Default::default()
                    },
                )?;
                let feed_put = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(feed_item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                let take_slot = if linked.already_occupies_slot {
                    None
                } else {
                    Some(self.take_match_slot(match_id, linked.counted_side_id.as_deref())?)
                };
                (
                    linked.item,
                    Some(feed_put),
                    Some(match_id.clone()),
                    take_slot,
                )
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
                (item, None, None, None)
            }
        };

        let counts_slot = take_slot.is_some();
        let mut put_roster = Put::builder()
            .table_name(self.table())
            .set_item(Some(roster_item));
        if counts_slot {
            // The "counted once" guard — see `accept_invitation_tx`.
            put_roster = put_roster
                .condition_expression("attribute_exists(#pk) AND #inv.#status <> :accepted")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_names("#inv", "invitation")
                .expression_attribute_names("#status", "status")
                .expression_attribute_values(":accepted", s("accepted"));
        }
        let put_roster = put_roster
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
        if let Some(take_slot) = take_slot {
            tx = tx.transact_items(TransactWriteItem::builder().update(take_slot).build());
        }

        match tx.send().await {
            Ok(_) => Ok(AcceptAttempt::Accepted(match_id)),
            // Only a counting attempt is worth retrying: the extra guard it
            // carries is the one a concurrent accept trips. Without it the
            // sole condition is the invitation still existing — it's revoked.
            Err(e) if super::is_transaction_conditional_failure(&e) && counts_slot => {
                Ok(AcceptAttempt::SlotGuardFailed)
            }
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("invitation {invitation_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// The meta-item update that takes one spot on a match for an accepting
    /// invitee: `ADD` 1 to `total_player_count`, and to `side_id`'s
    /// `player_count` when given. Conditioned on the match existing, so one
    /// deleted since the read isn't re-created as a bare counter stub.
    fn take_match_slot(&self, match_id: &str, side_id: Option<&str>) -> DaoResult<Update> {
        let update = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":one", AttributeValue::N("1".into()));
        // A single `ADD` section, comma-separated — DynamoDB rejects an
        // `UpdateExpression` with more than one `ADD` keyword.
        let update = match side_id {
            Some(side_id) => update
                .update_expression("ADD total_player_count :one, sides.#sid.player_count :one")
                .expression_attribute_names("#sid", side_id),
            None => update.update_expression("ADD total_player_count :one"),
        };
        update.build().map_err(|e| DaoError::Dynamo(e.to_string()))
    }

    /// Build the linked match-player item (external → user) for
    /// `invitation_id`, together with what the accept paths need from the same
    /// read (see [`LinkedMatchPlayer`]).
    /// Keeps the stable `player_id` so score references survive the flip.
    async fn linked_match_player_item(
        &self,
        match_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<LinkedMatchPlayer> {
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

        let already_occupies_slot = player.occupies_slot();
        player.user_id = Some(accepting_user_id.to_string());
        player.display_name = None;
        if let Some(inv) = player.invitation.as_mut() {
            inv.status = "accepted".to_string();
            inv.responded_at = Some(responded_at.to_string());
        }
        let side_id = player.side_id.clone();
        let counted_side_id = side_id
            .clone()
            .filter(|sid| agg.match_.sides.contains_key(sid));

        Ok(LinkedMatchPlayer {
            item: self.match_player_item(match_id, &player)?,
            starts_at,
            side_id,
            already_occupies_slot,
            counted_side_id,
        })
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
    /// Takes no spot. The saga only starts off the standalone invitation's
    /// transition into "accepted", which the synchronous accept writes in the
    /// same transaction that counts the spot — so by now it's counted, and
    /// counting here too would count it twice. (Were a row ever flipped to
    /// accepted without that, the saga's recount right after this —
    /// `refresh_side_roster_previews` — would still bring the counts in line.)
    ///
    /// Returns `NotFound` if the invitation or its target entry is gone.
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
                let linked = self
                    .linked_match_player_item(
                        match_id,
                        invitation_id,
                        accepting_user_id,
                        responded_at,
                    )
                    .await?;
                self.put_item(linked.item).await
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
