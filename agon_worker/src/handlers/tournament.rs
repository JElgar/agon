//! Inline handler: knockout bracket progression.
//!
//! On any match `#META` write with a confirmed score, if that match belongs
//! to a tournament and has a bracket edge (`TournamentBracketEdgeRecord` —
//! written once at generation time, in `agon_service`'s `start` handler, for
//! every non-final knockout match), propagate the winner into the next
//! match's side. Idempotent via `Dao::fill_tournament_bracket_side`'s
//! "still empty" guard — SQS redelivery, or any later edit to an
//! already-propagated match, is a harmless no-op. League tournaments have
//! no bracket edges at all, so this is a no-op for them too.

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};

use crate::error::WorkerResult;
use crate::event::ChangeEvent;

/// Handle a tournament-progression-relevant change event: any non-remove
/// write to a match's `#META`. Everything else — a match outside a
/// tournament, a league match, an unscored match, or a knockout final (no
/// edge to follow) — is a no-op.
pub async fn handle(dao: &Dao, ev: &ChangeEvent) -> WorkerResult<()> {
    if ev.kind.is_remove() {
        return Ok(());
    }
    let (Pk::Match(match_id), Sk::Meta) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };

    let Some(match_) = dao.get_match(match_id).await?.map(|agg| agg.match_) else {
        return Ok(());
    };
    let Some(tournament_id) = match_.tournament_id.clone() else {
        return Ok(());
    };
    // No winner yet (score not confirmed), or a draw — a knockout match
    // isn't expected to end in one; if it somehow does, it just never
    // advances, same as leaving the match unscored.
    let Some(winner_side_id) = match_
        .confirmed_score
        .as_ref()
        .and_then(|cs| cs.winner_side_id.clone())
    else {
        return Ok(());
    };
    let Some(winner_team_id) = match_
        .sides
        .get(&winner_side_id)
        .and_then(|s| s.team_id.clone())
    else {
        return Ok(());
    };

    let Some(edge) = dao
        .get_tournament_bracket_edge(&tournament_id, match_id)
        .await?
    else {
        return Ok(());
    };
    let (Some(into_match_id), Some(into_side_id)) = (
        edge.winner_feeds_into_match_id.as_ref(),
        edge.winner_feeds_into_side.as_ref(),
    ) else {
        return Ok(());
    };

    dao.fill_tournament_bracket_side(into_match_id, into_side_id, &winner_team_id)
        .await?;
    Ok(())
}
