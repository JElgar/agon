//! Inline handler: reconcile per-user, per-sport stats from a match's state.
//!
//! On any write to a match's `#META`, we recompute what the match *currently*
//! contributes to each participant's stats and reconcile it (see
//! [`Dao::reconcile_match_contribution`]): a completed match contributes
//! `played` for everyone who actually played and `won` for the winning side;
//! anything else (scheduled, cancelled, roster/score change) is reconciled to
//! its new value, backing out stale contributions.
//!
//! **Idempotency / correctness**: the worker sees a match-meta event on *every*
//! write to it (status changes, but also each like/comment counter bump), and
//! SQS delivers at-least-once. Reconciliation is a diff against the stored
//! per-match contribution, so an unchanged state writes nothing, and a changed
//! one (re-score, cancellation, roster edit) self-corrects. We reconcile the
//! union of current participants and users with an existing contribution, so a
//! player removed from the roster has their contribution backed out too.

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};

use crate::error::WorkerResult;
use crate::event::ChangeEvent;

/// Handle a stats-relevant change event: any non-remove write to a match's
/// `#META`. Everything else is ignored.
pub async fn handle(dao: &Dao, ev: &ChangeEvent) -> WorkerResult<()> {
    if ev.kind.is_remove() {
        return Ok(());
    }
    let (Pk::Match(match_id), Sk::Meta) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };
    reconcile_match_stats(dao, match_id).await
}

/// Reconcile every participant's per-sport stat contribution against a match's
/// current state. Shared by the `#META` stream handler and the accept saga: a
/// roster link (a `PLAYER#` write) doesn't touch `#META`, so accepting an invite
/// into an already-completed match must reconcile the newly-linked player here.
///
/// Idempotent (a diff against the stored contribution), so re-running it — from
/// either caller, or a redelivery — converges to the same state.
pub async fn reconcile_match_stats(dao: &Dao, match_id: &str) -> WorkerResult<()> {
    // Re-read the current aggregate rather than trusting the stream image, so we
    // reflect the latest committed sides/players/score. A missing match means
    // there's nothing to attribute (its contributions, if any, are orphaned but
    // harmless — a delete flow would clean them up).
    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };

    let completed = agg.match_.status == "completed";
    let sport = agg.match_.match_type.clone();
    // The confirmed winner's side, if a score has been agreed.
    let winner_side_id = agg
        .match_
        .confirmed_score
        .as_ref()
        .and_then(|cs| cs.winner_side_id.clone());

    // Desired `won` per participant who actually played, keyed by user id.
    // "Played" = a completed match where the player is the creator/self-added
    // (no embedded invitation) or an accepted invitee. Pending/declined invitees
    // are on the roster but didn't play.
    let mut desired: std::collections::BTreeMap<String, bool> = Default::default();
    if completed {
        for player in &agg.players {
            let Some(user_id) = &player.user_id else {
                continue;
            };
            let played = match &player.invitation {
                None => true,
                Some(inv) => inv.status == "accepted",
            };
            if !played {
                continue;
            }
            let won = match (&player.side_id, &winner_side_id) {
                (Some(side), Some(winner)) => side == winner,
                _ => false,
            };
            // If a user somehow appears twice, a win on either side counts.
            let entry = desired.entry(user_id.clone()).or_insert(false);
            *entry = *entry || won;
        }
    }

    // Reconcile the union of current participants and anyone who already has a
    // stored contribution (so removed players / a now-uncompleted match get
    // backed out to zero).
    let mut targets: std::collections::BTreeSet<String> = desired.keys().cloned().collect();
    for uid in dao.list_stat_contribution_user_ids(match_id).await? {
        targets.insert(uid);
    }

    for user_id in targets {
        let won = desired.get(&user_id).copied().unwrap_or(false);
        let played = desired.contains_key(&user_id);
        dao.reconcile_match_contribution(match_id, &user_id, &sport, played, won)
            .await?;
    }

    Ok(())
}
