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

    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };
    let match_ = &agg.match_;
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
    let Some(winner_side) = match_.sides.get(&winner_side_id) else {
        return Ok(());
    };
    let Some(winner_team_id) = winner_side.team_id.clone() else {
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

    let Some(tournament) = dao.get_tournament(&tournament_id).await? else {
        return Ok(());
    };

    if tournament.entry_audience == agon_core::dao::records::TournamentEntryAudienceRecord::Players
    {
        // `winner_team_id` is a synthetic squad id here, not a real team —
        // there's no team membership to self-join from, so the squad's
        // actual roster (whoever played this match for the winning side)
        // has to be copied across explicitly.
        // A squad side always carries its squad's name/colour — both
        // `start_tournament` and a manually added match copy them from
        // `TournamentRecord::squad_label` — so there's nothing to fall back
        // to here; a side without them is a bug upstream, not a default.
        let (Some(squad_name), Some(squad_colour)) =
            (winner_side.name.clone(), winner_side.colour.clone())
        else {
            tracing::error!(
                match_id = %into_match_id,
                squad_id = %winner_team_id,
                "squad side has no name/colour; not filling the next bracket slot"
            );
            return Ok(());
        };
        let filled = dao
            .fill_tournament_bracket_squad_side(
                into_match_id,
                into_side_id,
                &winner_team_id,
                &squad_name,
                &squad_colour,
            )
            .await?;
        if filled {
            for player in &agg.players {
                if player.side_id.as_deref() != Some(winner_side_id.as_str()) {
                    continue;
                }
                let copied = agon_core::dao::records::MatchPlayerRecord {
                    player_id: agon_core::id::new_id(),
                    user_id: player.user_id.clone(),
                    display_name: player.display_name.clone(),
                    side_id: Some(into_side_id.clone()),
                    is_member_of_team: None,
                    invitation: None,
                    joined_via: None,
                };
                dao.add_match_player_tx(into_match_id, &copied).await?;
            }
        }
    } else {
        dao.fill_tournament_bracket_side(into_match_id, into_side_id, &winner_team_id)
            .await?;
    }
    Ok(())
}
