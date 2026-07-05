//! Inline handler: generate notifications from social events.
//!
//! Triggered by newly-created social edges: a follow, a match like, or a match
//! comment. For each we synthesise a `NotificationRecord` for the target user
//! and write it via the DAO (which bumps the unread badge atomically).
//!
//! **Idempotency**: notification ids are **deterministic**, derived from the
//! source item's keys (`notif-<kind>-<...>`). Combined with the guarded,
//! id-idempotent `create_notification`, a redelivered stream event re-computes
//! the same id and the write is a harmless no-op — no duplicate bell entries, no
//! double-counted badge. We only act on `INSERT` (the edge's creation); modifies
//! and removes don't produce notifications.

use agon_core::dao::Dao;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{NotificationKindRecord, NotificationRecord};

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
    // Notifications are generated only on the creation of the edge.
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
        _ => Ok(()),
    }
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
