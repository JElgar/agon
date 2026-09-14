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
//!   headcounts, or puts them on its waitlist if it's full. Follower fan-out
//!   stays async (the stream event this transaction produces starts the
//!   fan-out saga).
//! - [`Dao::link_accepted_invitation`] — the **saga** re-link (external → user)
//!   used by the async accept workflow. Kept for at-least-once replay; it is a
//!   fixed-point re-write of the same accepted state as the transaction above,
//!   and never touches the headcounts.
//!
//! Idempotent: re-running either against an already-accepted entry re-writes the
//! same accepted state — taking no second spot, and leaving a waitlisted
//! accepter waiting — so the at-least-once accept workflow (or a repeated
//! accept request) can replay safely.

use aws_sdk_dynamodb::types::{Put, TransactWriteItem};

use super::audience::AudienceMember;
use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, Item, s};
use super::match_ops::{SlotCaps, take_slot_update};
use super::records::{InvitationContextRecord, MatchPlayerRecord};

/// How many read-then-write attempts [`Dao::accept_invitation_tx`] makes. An
/// accept normally takes one. Into a full match it takes two (the spot, then
/// the waitlist), or three if a recount found room that was gone again by the
/// second try. The rest is for a roster row that changes under us, such as a
/// concurrent accept of the same invite: the retry re-reads the row accepted
/// and re-writes it without counting. The bound only stops a row that keeps
/// changing from looping.
const ACCEPT_ATTEMPTS: u32 = 5;

/// What [`Dao::accept_invitation_tx`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct AcceptOutcome {
    /// The match id for a match invite; `None` for a team invite.
    pub match_id: Option<String>,
    /// Whether the accepter is on the match's waitlist rather than in a spot.
    /// The row's state, not just this call's doing, so accepting an invite
    /// that was waitlisted again says so again (unless they've been moved in
    /// since). Always `false` for a team invite.
    pub waitlisted: bool,
}

/// Which write an accept attempt makes for a roster row that isn't accepted
/// yet.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AcceptInto {
    /// Take a spot, with the cap-guarded `ADD`.
    Slot,
    /// The match is full (its cap guard failed, and a recount agreed): the
    /// waitlist, counting nothing.
    Waitlist,
}

/// The outcome of one [`Dao::try_accept_invitation`] attempt.
enum AcceptAttempt {
    /// Committed.
    Accepted(AcceptOutcome),
    /// The roster row changed since it was read — most likely a concurrent
    /// accept of the same invite got there first. Nothing was written; re-read
    /// and try again.
    RowChanged,
    /// Taking a spot failed its cap guard. Nothing was written;
    /// [`Dao::accept_invitation_tx`] decides between another try and the
    /// waitlist.
    Full { match_id: String, caps: SlotCaps },
}

/// A match roster row rebuilt as linked and accepted, plus what the accept
/// paths need from the same read. See [`Dao::linked_match_player`].
struct LinkedMatchPlayer {
    player: MatchPlayerRecord,
    /// The match's `starts_at` — feed sort material.
    starts_at: String,
    /// Whether the row *as read* was already accepted — i.e. this accept is a
    /// re-write of an accepted invite, not the one that answers it (and so
    /// takes a spot, or a waitlist place).
    ///
    /// This used to be `MatchPlayerRecord::occupies_slot`, and for an invited
    /// row the two were the same test until the waitlist: an accepted invitee
    /// who is waiting holds no spot, but has already been accepted, and
    /// accepting again mustn't count them.
    already_accepted: bool,
    /// The caps for a spot on the player's side (see [`SlotCaps::for_side`],
    /// which also covers a side id the match no longer has).
    caps: SlotCaps,
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
    ///    `player_count` if they're on one, guarded on the caps exactly as a
    ///    join is (`take_slot_update`).
    ///
    /// The spot is counted here, in the accept itself, rather than left to the
    /// accept saga's later recount (`refresh_side_roster_previews`), so the
    /// counts are right the moment the accept returns — a joiner right behind
    /// it can't be let into the spot it just took. It's an atomic `ADD` for the
    /// same reason `join_match_tx`'s is: a count computed from a read and
    /// written back would race.
    ///
    /// **Into a full match.** If the cap guard fails, the match is recounted
    /// (`recount_has_room`, which heals counts that only look full) and a spot
    /// that turns up is tried for once more. Otherwise the accept goes through
    /// without step 4, with the roster row written `waitlisted_at`: accepted,
    /// and on the waitlist. The invite is accepted either way, because the
    /// invitee said yes and leaving it pending would keep asking them. Which
    /// guard failed is read off the transaction's cancellation reasons, not a
    /// re-read of the match, which (being eventually consistent) could still
    /// show the spot as free.
    ///
    /// **Counted once.** Accepting an already-accepted invite (a double tap, a
    /// retried request) stays the idempotent re-write it always was: it takes
    /// no second spot, and leaves a waitlisted accepter waiting. Step 4 (or the
    /// waitlist) only applies when the roster row as read isn't accepted, and
    /// the roster write is then conditioned on it *still* not being accepted.
    /// If a concurrent accept of the same invite commits in between, this
    /// transaction is cancelled whole — no `ADD` — and retried from a fresh
    /// read, which finds the row accepted and re-writes it without counting.
    /// The guard says "not accepted" rather than "pending" because what's
    /// being counted is the invite being answered, whatever its status was
    /// before. (As a side effect it also stops a counting accept re-creating a
    /// roster row removed since the read.) A re-write of a row that was
    /// already accepted is guarded on its waitlist state being as read
    /// instead, so a stale re-write can't put back a `waitlisted_at` that
    /// moving the player in (`move_in_waitlisted_player`) has just cleared,
    /// leaving them counted but shown as waiting.
    ///
    /// `NotFound` if the invitation or its embedding roster entry is gone.
    #[tracing::instrument(skip(self))]
    pub async fn accept_invitation_tx(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
    ) -> DaoResult<AcceptOutcome> {
        let mut into = AcceptInto::Slot;
        let mut recounted = false;
        for attempt in 0..ACCEPT_ATTEMPTS {
            super::batch::backoff(attempt).await;
            match self
                .try_accept_invitation(invitation_id, accepting_user_id, responded_at, now, into)
                .await?
            {
                AcceptAttempt::Accepted(outcome) => return Ok(outcome),
                AcceptAttempt::RowChanged => continue,
                AcceptAttempt::Full { match_id, caps } => {
                    // Recount before the first "full" is believed; after that,
                    // a failed guard is a lost race for the spot the recount
                    // found, and the accepter waits like anyone else.
                    if !recounted {
                        recounted = true;
                        if self.recount_has_room(&match_id, &caps).await? {
                            continue;
                        }
                    }
                    into = AcceptInto::Waitlist;
                }
            }
        }
        // Cancelled every time: the roster row or invitation keeps changing
        // under us, or has gone and our re-reads haven't caught up. Report it
        // the way a revoked invite is reported.
        Err(DaoError::NotFound(format!("invitation {invitation_id}")))
    }

    /// One read-then-write attempt at [`Self::accept_invitation_tx`], making
    /// `into`'s write if the roster row isn't accepted yet.
    async fn try_accept_invitation(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
        into: AcceptInto,
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
        //    feed row and — unless they'd already accepted — their spot, or
        //    their place on the waitlist.
        let (put_roster, feed_put, take_slot, outcome) = match &inv.context {
            InvitationContextRecord::Match { match_id, .. } => {
                let mut linked = self
                    .linked_match_player(match_id, invitation_id, accepting_user_id, responded_at)
                    .await?;
                let take_slot = match into {
                    _ if linked.already_accepted => None,
                    AcceptInto::Slot => Some((
                        take_slot_update(self.table(), match_id, &linked.caps)?,
                        linked.caps.clone(),
                    )),
                    AcceptInto::Waitlist => {
                        linked.player.waitlisted_at = Some(now.to_string());
                        None
                    }
                };
                let feed_item = self.feed_item(
                    accepting_user_id,
                    match_id,
                    &linked.starts_at,
                    now,
                    &AudienceMember {
                        viewer_side_id: linked.player.side_id.clone(),
                        ..Default::default()
                    },
                )?;
                let feed_put = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(feed_item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                let outcome = AcceptOutcome {
                    match_id: Some(match_id.clone()),
                    waitlisted: linked.player.waitlisted_at.is_some(),
                };
                (
                    self.accepted_match_player_put(match_id, &linked)?,
                    Some(feed_put),
                    take_slot,
                    outcome,
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
                let put_roster = Put::builder()
                    .table_name(self.table())
                    .set_item(Some(item))
                    .build()
                    .map_err(|e| DaoError::Dynamo(e.to_string()))?;
                let outcome = AcceptOutcome {
                    match_id: None,
                    waitlisted: false,
                };
                (put_roster, None, None, outcome)
            }
        };

        // Item order matters: failures are told apart by index below — 0 the
        // invitation, 1 the roster row, 2 the cap guard (when there is one).
        let mut tx = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_inv).build())
            .transact_items(TransactWriteItem::builder().put(put_roster).build());
        let mut slot_caps = None;
        if let Some((update, caps)) = take_slot {
            tx = tx.transact_items(TransactWriteItem::builder().update(update).build());
            slot_caps = Some(caps);
        }
        if let Some(feed_put) = feed_put {
            tx = tx.transact_items(TransactWriteItem::builder().put(feed_put).build());
        }

        match tx.send().await {
            Ok(_) => Ok(AcceptAttempt::Accepted(outcome)),
            // The invitation's gone: revoked.
            Err(e) if super::item_condition_failed(&e, 0) => {
                Err(DaoError::NotFound(format!("invitation {invitation_id}")))
            }
            // Checked before the cap guard: if a concurrent accept of this
            // invite both answered it and took the last spot, the retry should
            // find it accepted, not put it on the waitlist.
            Err(e) if super::item_condition_failed(&e, 1) => Ok(AcceptAttempt::RowChanged),
            Err(e) if slot_caps.is_some() && super::item_condition_failed(&e, 2) => {
                Ok(AcceptAttempt::Full {
                    match_id: outcome.match_id.unwrap_or_default(),
                    caps: slot_caps.unwrap_or_default(),
                })
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// The roster write for a match accept, guarded on the row being as read
    /// (see `accept_invitation_tx`'s "Counted once"): not yet accepted, when
    /// this accept is the one answering the invite; otherwise, waiting or not
    /// just as it was.
    fn accepted_match_player_put(
        &self,
        match_id: &str,
        linked: &LinkedMatchPlayer,
    ) -> DaoResult<Put> {
        let put = Put::builder()
            .table_name(self.table())
            .set_item(Some(self.match_player_item(match_id, &linked.player)?))
            .expression_attribute_names("#pk", ATTR_PK);
        let put = if !linked.already_accepted {
            put.condition_expression("attribute_exists(#pk) AND #inv.#status <> :accepted")
                .expression_attribute_names("#inv", "invitation")
                .expression_attribute_names("#status", "status")
                .expression_attribute_values(":accepted", s("accepted"))
        } else if linked.player.waitlisted_at.is_some() {
            put.condition_expression("attribute_exists(#pk) AND attribute_exists(waitlisted_at)")
        } else {
            put.condition_expression(
                "attribute_exists(#pk) AND attribute_not_exists(waitlisted_at)",
            )
        };
        put.build().map_err(|e| DaoError::Dynamo(e.to_string()))
    }

    /// The match player embedding `invitation_id`, rebuilt as linked (external
    /// → user) and accepted, together with what the accept paths need from the
    /// same read (see [`LinkedMatchPlayer`]).
    /// Keeps the stable `player_id` so score references survive the flip.
    async fn linked_match_player(
        &self,
        match_id: &str,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
    ) -> DaoResult<LinkedMatchPlayer> {
        let Some(agg) = self.get_match(match_id).await? else {
            return Err(DaoError::NotFound(format!("match {match_id}")));
        };

        let Some(mut player) = agg
            .players
            .iter()
            .find(|p| p.invitation.as_ref().is_some_and(|i| i.id == invitation_id))
            .cloned()
        else {
            return Err(DaoError::NotFound(format!(
                "no player for invitation {invitation_id} in match {match_id}"
            )));
        };

        let already_accepted = player
            .invitation
            .as_ref()
            .is_some_and(|inv| inv.status == "accepted");
        let caps = SlotCaps::for_side(&agg, player.side_id.as_deref());
        player.user_id = Some(accepting_user_id.to_string());
        player.display_name = None;
        if let Some(inv) = player.invitation.as_mut() {
            inv.status = "accepted".to_string();
            inv.responded_at = Some(responded_at.to_string());
        }

        Ok(LinkedMatchPlayer {
            player,
            starts_at: agg.match_.starts_at,
            already_accepted,
            caps,
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
    /// Nor does it move anyone on or off the waitlist: it re-writes the row
    /// with whatever `waitlisted_at` it was read with.
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
                    .linked_match_player(match_id, invitation_id, accepting_user_id, responded_at)
                    .await?;
                self.put_item(self.match_player_item(match_id, &linked.player)?)
                    .await
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
