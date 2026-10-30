//! Inline handler: reconcile per-user, per-sport stats from a match's state.
//!
//! On any write to a match's `#META`, we recompute what the match *currently*
//! contributes to each participant's stats and reconcile it (see
//! [`Dao::reconcile_match_contribution`]): a match with a score set
//! contributes an outcome (won/drawn/lost) for everyone who actually played,
//! plus any sport-specific counters (cricket runs/wickets/fours/sixes/
//! catches/balls faced/balls bowled/runs conceded/dismissals, football
//! goals/assists) derived from the score's per-player box score; anything
//! else (scheduled, cancelled, no score yet, roster change) is reconciled to
//! its new value, backing out stale contributions.
//!
//! **Idempotency / correctness**: the worker sees a match-meta event on *every*
//! write to it (status changes, but also each like/comment counter bump), and
//! SQS delivers at-least-once. Reconciliation is a diff against the stored
//! per-match contribution, so an unchanged state writes nothing, and a changed
//! one (re-score, cancellation, roster edit) self-corrects. We reconcile the
//! union of current participants and users with an existing contribution, so a
//! player removed from the roster has their contribution backed out too.

use std::collections::{BTreeMap, BTreeSet};

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::stats::{MatchContribution, MatchOutcome};

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

    // A match only contributes to stats once its score is set — otherwise
    // counting it would inflate `matches_played` with no corresponding win,
    // dragging down (or zeroing) the win rate for a game with no result yet.
    let confirmed_score = agg.match_.confirmed_score.as_ref();
    let sport = agg.match_.match_type.clone();
    let winner_side_id = confirmed_score.and_then(|cs| cs.winner_side_id.clone());

    // Desired contribution per participant who actually played, keyed by user
    // id. "Played" = a match with a confirmed score where the player took a
    // spot (`MatchPlayerRecord::occupies_slot` — the same rule the roster
    // counts use). Pending/declined invitees are on the roster but didn't play.
    let mut desired: BTreeMap<String, MatchContribution> = Default::default();
    if let Some(cs) = confirmed_score {
        // Each user's roster entries in this match (almost always one) and
        // their best outcome across them (won > drawn > lost).
        let mut played: BTreeMap<&str, (MatchOutcome, Vec<&str>)> = Default::default();
        for player in &agg.players {
            let Some(user_id) = &player.user_id else {
                continue;
            };
            if !player.occupies_slot() {
                continue;
            }
            // A player with no side yet (still waiting to be assigned one)
            // didn't play for either side, so they get no outcome and no
            // box-score contribution for this match at all — not a loss.
            let Some(side_id) = &player.side_id else {
                continue;
            };
            // No winner recorded => the match was a draw for everyone who
            // played, regardless of side. Otherwise won/lost depends on
            // whether this player's side matches the winner.
            let outcome = if winner_side_id.is_none() {
                MatchOutcome::Drawn
            } else if Some(side_id) == winner_side_id.as_ref() {
                MatchOutcome::Won
            } else {
                MatchOutcome::Lost
            };
            let (best, player_ids) = played
                .entry(user_id.as_str())
                .or_insert((outcome, Vec::new()));
            *best = match (*best, outcome) {
                (MatchOutcome::Won, _) | (_, MatchOutcome::Won) => MatchOutcome::Won,
                (MatchOutcome::Drawn, _) | (_, MatchOutcome::Drawn) => MatchOutcome::Drawn,
                _ => MatchOutcome::Lost,
            };
            player_ids.push(&player.player_id);
        }

        for (user_id, (outcome, player_ids)) in played {
            let contribution = agon_core::sports::contribution(
                &sport,
                &cs.score,
                &player_ids,
                agg.match_.format.as_ref(),
                match_id,
            );
            desired.insert(
                user_id.to_string(),
                MatchContribution {
                    outcome: Some(outcome),
                    counters: contribution.counters,
                    bests: contribution.bests,
                },
            );
        }
    }

    // Reconcile the union of current participants and anyone who already has a
    // stored contribution (so removed players / a now-uncompleted match get
    // backed out to zero).
    let mut targets: BTreeSet<String> = desired.keys().cloned().collect();
    for uid in dao.list_stat_contribution_user_ids(match_id).await? {
        targets.insert(uid);
    }

    for user_id in targets {
        let contribution = desired.get(&user_id).cloned().unwrap_or_default();
        dao.reconcile_match_contribution(match_id, &user_id, &sport, &contribution)
            .await?;
    }

    Ok(())
}
