//! Inline handler: generate notifications from social events.
//!
//! Triggered by newly-created social edges: a follow, a match like, a match
//! comment, an invitation, or a score submission. For each we synthesise a
//! `NotificationRecord` for the target user and write it via the DAO (which
//! bumps the unread badge atomically).
//!
//! **Idempotency**: notification ids are **deterministic**, derived from the
//! source item's keys (`notif-<kind>-<...>`). Combined with the guarded,
//! id-idempotent `create_notification`, a redelivered stream event re-computes
//! the same id and the write is a harmless no-op — no duplicate bell entries, no
//! double-counted badge. Most notifications act only on `INSERT` (the edge's
//! creation). The one exception is score submissions: the submission item is
//! overwritten in place when a side responds, so "score confirmed" is detected
//! as a `pending → confirmed` MODIFY transition (fired once, on the transition).

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{
    InvitationContextRecord, InvitationRecord, NotificationKindRecord, NotificationRecord,
    ScoreSubmissionRecord,
};

use crate::error::{WorkerError, WorkerResult};
use crate::event::{ChangeEvent, ChangeKind};

/// Handle a notification-relevant change event. `now` is an RFC3339 timestamp
/// stamped by the worker when it processes the message: the thin envelope
/// carries no timestamp and follow/like edges don't expose a getter, so the
/// notification's `created_at` (which drives GSI1 time ordering) is set at
/// processing time. This is stable under redelivery because the notification id
/// is deterministic — a duplicate is a no-op, so the first-processed timestamp
/// wins and never changes.
pub async fn handle(dao: &Dao, ev: &ChangeEvent, now: &str) -> WorkerResult<()> {
    // Score submissions are the one case that reacts to a MODIFY as well as an
    // INSERT: the submission item is overwritten in place when a side confirms,
    // so the pending → confirmed transition is a MODIFY. Handle it before the
    // INSERT-only guard below.
    if let (Pk::Match(match_id), Sk::ScoreSubmission(submission_id)) = (&ev.pk, &ev.sk) {
        return notify_score(dao, ev, match_id, submission_id, now).await;
    }

    // Invitations, like score submissions, react to both INSERT (the invite was
    // sent → notify the invitee) and MODIFY (a `pending → accepted` transition →
    // notify the inviter). Handle before the INSERT-only guard below.
    if let (Pk::Invitation(invitation_id), Sk::Meta) = (&ev.pk, &ev.sk) {
        return notify_invitation_event(dao, ev, invitation_id, now).await;
    }

    // Every other notification is generated only on the creation of the edge.
    if ev.kind != ChangeKind::Insert {
        return Ok(());
    }

    match (&ev.pk, &ev.sk) {
        // A user gained a follower: notify the followed user.
        (Pk::User(followee_id), Sk::Follower(follower_id)) => {
            notify_follow(dao, followee_id, follower_id, now).await
        }
        // A match was liked: notify the match participants.
        (Pk::Match(match_id), Sk::Like(liker_id)) => {
            notify_like(dao, match_id, liker_id, now).await
        }
        // A top-level comment on a match: notify the participants.
        (Pk::Match(match_id), Sk::Comment(comment_id)) => {
            notify_comment(dao, match_id, comment_id).await
        }
        // A reply to a comment: notify the parent comment's author and the match
        // participants.
        (Pk::Match(match_id), Sk::Reply(reply_id)) => notify_reply(dao, match_id, reply_id).await,
        _ => Ok(()),
    }
}

/// Dispatch an invitation change event. INSERT (invite sent) notifies the
/// invitee; MODIFY notifies the inviter when the invitation transitions into
/// `accepted`. A decline, a revoke (REMOVE), or any other modify is ignored.
async fn notify_invitation_event(
    dao: &Dao,
    ev: &ChangeEvent,
    invitation_id: &str,
    now: &str,
) -> WorkerResult<()> {
    match ev.kind {
        ChangeKind::Insert => notify_invitation(dao, ev, invitation_id, now).await,
        ChangeKind::Modify => notify_invitation_accepted(dao, ev, invitation_id, now).await,
        ChangeKind::Remove => Ok(()),
    }
}

/// An invitation transitioned into "accepted": notify the inviter that their
/// invitee joined. Fires only on the `!accepted → accepted` transition, so a
/// later re-modify of an already-accepted invitation is a no-op (idempotent
/// alongside the deterministic notification id).
async fn notify_invitation_accepted(
    dao: &Dao,
    ev: &ChangeEvent,
    invitation_id: &str,
    now: &str,
) -> WorkerResult<()> {
    let Some(inv) = ev.new_record::<InvitationRecord>() else {
        return Ok(());
    };
    if inv.status != "accepted" {
        return Ok(());
    }
    // Only fire on the transition into "accepted" — not on a modify of an
    // already-accepted invitation (e.g. an unrelated field rewrite / redelivery).
    let was_accepted = ev
        .old_record::<InvitationRecord>()
        .map(|old| old.status == "accepted")
        .unwrap_or(false);
    if was_accepted {
        return Ok(());
    }

    // The accepter: the invited user for a user-kind invite. Token/external
    // invites carry no linked user id at accept time (the external member is
    // reconciled to an account separately), so there's no coherent actor to
    // attribute — skip until that linking exists.
    let Some(accepter_user_id) = inv.invited_user_id.clone() else {
        return Ok(());
    };
    // Never notify the inviter about accepting their own invite (degenerate
    // self-invite).
    if accepter_user_id == inv.invited_by_user_id {
        return Ok(());
    }

    let notif = NotificationRecord {
        // Deterministic id → idempotent under redelivery (one row per invite).
        id: format!("notif-invitationaccepted-{invitation_id}"),
        user_id: inv.invited_by_user_id.clone(),
        is_read: false,
        created_at: now.to_string(),
        kind: NotificationKindRecord::InvitationAccepted {
            actor_user_id: accepter_user_id,
            invitation_id: invitation_id.to_string(),
            context: inv.context.clone(),
        },
    };
    dao.create_notification(&notif).await?;
    Ok(())
}

async fn notify_invitation(
    dao: &Dao,
    ev: &ChangeEvent,
    invitation_id: &str,
    now: &str,
) -> WorkerResult<()> {
    // Read the invitation from the stream image (the write we're reacting to).
    let Some(inv) = ev.new_record::<InvitationRecord>() else {
        return Ok(());
    };

    // Only user-kind invitations have an Agon user to notify. Token/external
    // invites are delivered out of band (link), so there's no in-app recipient.
    let Some(invited_user_id) = inv.invited_user_id.clone() else {
        return Ok(());
    };

    // Never notify the match/team creator about their own invite — the invitee
    // is the recipient, the creator is the actor. (Also covers the degenerate
    // self-invite case.)
    if invited_user_id == inv.invited_by_user_id {
        return Ok(());
    }

    let kind = match &inv.context {
        InvitationContextRecord::Match {
            match_id,
            match_name,
        } => NotificationKindRecord::MatchInvitation {
            actor_user_id: inv.invited_by_user_id.clone(),
            invitation_id: invitation_id.to_string(),
            match_id: match_id.clone(),
            match_name: match_name.clone(),
        },
        InvitationContextRecord::Team { team_id, team_name } => {
            NotificationKindRecord::TeamInvitation {
                actor_user_id: inv.invited_by_user_id.clone(),
                invitation_id: invitation_id.to_string(),
                team_id: team_id.clone(),
                team_name: team_name.clone(),
            }
        }
    };

    let notif = NotificationRecord {
        // Deterministic id → idempotent under redelivery (one row per invite).
        id: format!("notif-invitation-{invitation_id}"),
        user_id: invited_user_id,
        is_read: false,
        created_at: now.to_string(),
        kind,
    };
    dao.create_notification(&notif).await?;
    Ok(())
}

async fn notify_follow(
    dao: &Dao,
    followee_id: &str,
    follower_id: &str,
    now: &str,
) -> WorkerResult<()> {
    // A self-follow edge should be impossible: the DAO's follow_user rejects
    // follower == followee, so such an edge can never commit or reach the stream.
    // If we see one, the write path's invariant has been broken — fail loudly.
    if followee_id == follower_id {
        return Err(WorkerError::Invariant(format!(
            "self-follow edge for user {followee_id}"
        )));
    }
    let notif = NotificationRecord {
        id: format!("notif-follow-{followee_id}-{follower_id}"),
        user_id: followee_id.to_string(),
        is_read: false,
        created_at: now.to_string(),
        kind: NotificationKindRecord::Follow {
            actor_user_id: follower_id.to_string(),
        },
    };
    dao.create_notification(&notif).await?;
    Ok(())
}

async fn notify_like(dao: &Dao, match_id: &str, liker_id: &str, now: &str) -> WorkerResult<()> {
    let Some(agg) = dao.get_match(match_id).await? else {
        // Match gone; nothing to notify about.
        return Ok(());
    };
    // Recipients: the match participants (players with a linked user), minus the
    // liker themselves. Deduplicated across sides.
    let recipients = participant_user_ids(&agg, liker_id);
    for user_id in recipients {
        let notif = NotificationRecord {
            id: format!("notif-like-{match_id}-{liker_id}-{user_id}"),
            user_id,
            is_read: false,
            created_at: now.to_string(),
            kind: NotificationKindRecord::Like {
                actor_user_id: liker_id.to_string(),
                match_id: match_id.to_string(),
                match_name: agg.match_.name.clone(),
            },
        };
        dao.create_notification(&notif).await?;
    }
    Ok(())
}

async fn notify_comment(dao: &Dao, match_id: &str, comment_id: &str) -> WorkerResult<()> {
    let Some(comment) = dao.get_comment(match_id, comment_id).await? else {
        return Ok(());
    };
    let Some(author_id) = comment.author_user_id.clone() else {
        return Ok(());
    };
    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };

    let preview = comment.text.as_deref().map(preview_of).unwrap_or_default();

    // Notify match participants (except the comment author). The comment carries
    // its own created_at, so the notification uses it directly.
    let recipients = participant_user_ids(&agg, &author_id);
    for user_id in recipients {
        let notif = NotificationRecord {
            id: format!("notif-comment-{match_id}-{comment_id}-{user_id}"),
            user_id,
            is_read: false,
            created_at: comment.created_at.clone(),
            kind: NotificationKindRecord::Comment {
                actor_user_id: author_id.clone(),
                match_id: match_id.to_string(),
                comment_id: comment_id.to_string(),
                preview: preview.clone(),
            },
        };
        dao.create_notification(&notif).await?;
    }
    Ok(())
}

/// A reply to a comment: notify the parent comment's author plus the match
/// participants (deduplicated), excluding the reply's author. The reply row is
/// fetched by its own id (`Sk::Reply`); its `parent_id` names the top-level
/// comment, whose author we resolve and include as a recipient.
async fn notify_reply(dao: &Dao, match_id: &str, reply_id: &str) -> WorkerResult<()> {
    let Some(reply) = dao.get_reply(match_id, reply_id).await? else {
        return Ok(());
    };
    let Some(author_id) = reply.author_user_id.clone() else {
        return Ok(());
    };
    let Some(parent_id) = reply.parent_id.clone() else {
        // A reply without a parent is malformed — the write path always sets it.
        return Err(WorkerError::Invariant(format!(
            "reply {reply_id} on match {match_id} has no parent_id"
        )));
    };
    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };

    let preview = reply.text.as_deref().map(preview_of).unwrap_or_default();

    // Recipients: match participants plus the parent comment's author, minus the
    // reply author. Deduplicated so the parent author (who may also be a
    // participant) is notified exactly once. Use a set for the union.
    let mut recipients: std::collections::BTreeSet<String> =
        participant_user_ids(&agg, &author_id).into_iter().collect();
    // The parent comment may be tombstoned (author cleared) or gone; only add a
    // live author, and never the reply's own author.
    if let Some(parent) = dao.get_comment(match_id, &parent_id).await?
        && let Some(parent_author) = parent.author_user_id
        && parent_author != author_id
    {
        recipients.insert(parent_author);
    }

    for user_id in recipients {
        let notif = NotificationRecord {
            id: format!("notif-reply-{match_id}-{reply_id}-{user_id}"),
            user_id,
            is_read: false,
            created_at: reply.created_at.clone(),
            kind: NotificationKindRecord::Reply {
                actor_user_id: author_id.clone(),
                match_id: match_id.to_string(),
                comment_id: reply_id.to_string(),
                parent_comment_id: parent_id.clone(),
                preview: preview.clone(),
            },
        };
        dao.create_notification(&notif).await?;
    }
    Ok(())
}

/// Dispatch a score-submission stream event. Submitted (INSERT) and confirmed
/// (MODIFY, pending → confirmed) are the two events we notify on; a dispute or
/// any other modify is ignored.
async fn notify_score(
    dao: &Dao,
    ev: &ChangeEvent,
    match_id: &str,
    submission_id: &str,
    now: &str,
) -> WorkerResult<()> {
    match ev.kind {
        ChangeKind::Insert => notify_score_submitted(dao, ev, match_id, submission_id, now).await,
        ChangeKind::Modify => notify_score_confirmed(dao, ev, match_id, submission_id, now).await,
        ChangeKind::Remove => Ok(()),
    }
}

/// A score was submitted: notify every participant except the submitter. Each
/// row carries `needs_confirmation` — true for a pending submission's opposing
/// sides (who must confirm/dispute), false for the submitter's own side-mates
/// and for a submission that arrived already confirmed (score set directly).
async fn notify_score_submitted(
    dao: &Dao,
    ev: &ChangeEvent,
    match_id: &str,
    submission_id: &str,
    now: &str,
) -> WorkerResult<()> {
    let Some(sub) = ev.new_record::<ScoreSubmissionRecord>() else {
        return Ok(());
    };
    let Some(agg) = dao.get_match(match_id).await? else {
        // Match gone; nothing to notify about.
        return Ok(());
    };

    // Resolve the submitter (a player id) to a user id and their side. If the
    // submitter isn't linked to a user we can't attribute or exclude them, so
    // there's nothing coherent to send.
    let submitter = agg
        .players
        .iter()
        .find(|p| p.player_id == sub.submitted_by_player_id);
    let Some(submitter_user_id) = submitter.and_then(|p| p.user_id.clone()) else {
        return Ok(());
    };
    let submitter_side_id = submitter.and_then(|p| p.side_id.clone());

    // A pending submission still needs the other side(s) to confirm; one that
    // arrives already confirmed (direct score set) is purely informational.
    let is_pending = sub.status == "pending";

    // Build the recipient set (deduplicated by user id, submitter excluded). A
    // user needs to confirm only if the submission is pending and they're not on
    // the submitter's side; if a user appears on multiple sides, err towards
    // asking them to confirm.
    let mut recipients: std::collections::BTreeMap<String, bool> =
        std::collections::BTreeMap::new();
    for player in &agg.players {
        let Some(uid) = &player.user_id else { continue };
        if *uid == submitter_user_id {
            continue;
        }
        let needs = is_pending && player.side_id != submitter_side_id;
        recipients
            .entry(uid.clone())
            .and_modify(|n| *n = *n || needs)
            .or_insert(needs);
    }

    for (user_id, needs_confirmation) in recipients {
        let notif = NotificationRecord {
            id: format!("notif-scoresubmitted-{match_id}-{submission_id}-{user_id}"),
            user_id,
            is_read: false,
            created_at: now.to_string(),
            kind: NotificationKindRecord::ScoreSubmitted {
                actor_user_id: submitter_user_id.clone(),
                match_id: match_id.to_string(),
                match_name: agg.match_.name.clone(),
                submission_id: submission_id.to_string(),
                needs_confirmation,
            },
        };
        dao.create_notification(&notif).await?;
    }
    Ok(())
}

/// A submission transitioned into "confirmed": notify the submitter that their
/// score is now confirmed. Fires only on the pending → confirmed transition so a
/// later re-modify of an already-confirmed submission is a no-op.
async fn notify_score_confirmed(
    dao: &Dao,
    ev: &ChangeEvent,
    match_id: &str,
    submission_id: &str,
    now: &str,
) -> WorkerResult<()> {
    let Some(new_sub) = ev.new_record::<ScoreSubmissionRecord>() else {
        return Ok(());
    };
    if new_sub.status != "confirmed" {
        return Ok(());
    }
    let was_confirmed = ev
        .old_record::<ScoreSubmissionRecord>()
        .map(|old| old.status == "confirmed")
        .unwrap_or(false);
    if was_confirmed {
        return Ok(());
    }

    let Some(agg) = dao.get_match(match_id).await? else {
        return Ok(());
    };

    // The submitter (who we notify).
    let Some(submitter_user_id) = agg
        .players
        .iter()
        .find(|p| p.player_id == new_sub.submitted_by_player_id)
        .and_then(|p| p.user_id.clone())
    else {
        return Ok(());
    };

    // The actor: the participant whose confirm response completed it — the most
    // recent confirm that isn't the submitter's own pre-seeded one.
    let confirmer_player_id = new_sub
        .responses
        .iter()
        .rev()
        .find(|r| {
            r.response == "confirm" && r.responded_by_player_id != new_sub.submitted_by_player_id
        })
        .map(|r| r.responded_by_player_id.clone());
    let Some(confirmer_player_id) = confirmer_player_id else {
        return Ok(());
    };
    let Some(confirmer_user_id) = agg
        .players
        .iter()
        .find(|p| p.player_id == confirmer_player_id)
        .and_then(|p| p.user_id.clone())
    else {
        return Ok(());
    };
    // Never notify the submitter about their own confirmation.
    if confirmer_user_id == submitter_user_id {
        return Ok(());
    }

    let notif = NotificationRecord {
        id: format!("notif-scoreconfirmed-{match_id}-{submission_id}"),
        user_id: submitter_user_id,
        is_read: false,
        created_at: now.to_string(),
        kind: NotificationKindRecord::ScoreConfirmed {
            actor_user_id: confirmer_user_id,
            match_id: match_id.to_string(),
            match_name: agg.match_.name.clone(),
            submission_id: submission_id.to_string(),
        },
    };
    dao.create_notification(&notif).await?;
    Ok(())
}

/// The deduplicated set of linked user ids among a match's players, excluding
/// `exclude` (typically the actor, who shouldn't be notified about their own
/// action).
fn participant_user_ids(
    agg: &agon_core::dao::match_ops::MatchAggregate,
    exclude: &str,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for player in &agg.players {
        if let Some(uid) = &player.user_id
            && uid != exclude
        {
            seen.insert(uid.clone());
        }
    }
    seen.into_iter().collect()
}

/// A short preview of comment text for the notification body.
fn preview_of(text: &str) -> String {
    const MAX: usize = 80;
    if text.chars().count() <= MAX {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(MAX).collect();
        format!("{truncated}…")
    }
}
