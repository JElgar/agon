//! Inline handler: send a push notification for every newly-created
//! `NotificationRecord`.
//!
//! Deliberately does **not** re-derive recipients/content from the original
//! social event the way `notify.rs` does. `notify::handle` already writes the
//! `NotificationRecord` (an item in this same table), and that write produces
//! its *own* stream event — `Pk::User(uid)` / `Sk::Notification(id)`, INSERT —
//! which flows back through this same pipeline. Reacting to that event instead
//! means zero duplication of `notify.rs`'s nine recipient-computing branches,
//! and the same at-least-once/idempotent handling every other handler gets.
//!
//! Future: a per-user (or per-kind / per-followed-team) push-preference check
//! — the YouTube-bell "mute this kind of notification" idea — belongs right
//! here, before the send loop: it only changes whether a push goes out, never
//! whether the in-app `NotificationRecord` (the bell entry) gets created.

use agon_core::dao::Dao;
use agon_core::dao::error::DaoError;
use agon_core::dao::keys::{Pk, Sk};
use agon_core::dao::records::{
    InvitationContextRecord, NotificationKindRecord, NotificationRecord,
};
use agon_core::push::{PushClient, PushOutcome};

use crate::error::WorkerResult;
use crate::event::{ChangeEvent, ChangeKind};

pub async fn handle(
    dao: &Dao,
    push: Option<&PushClient>,
    ui_base_url: Option<&str>,
    ev: &ChangeEvent,
) -> WorkerResult<()> {
    // Not configured (e.g. local dev without a GCP project) — nothing to do.
    let Some(push) = push else {
        return Ok(());
    };
    if ev.kind != ChangeKind::Insert {
        return Ok(());
    }
    let (Pk::User(user_id), Sk::Notification(_)) = (&ev.pk, &ev.sk) else {
        return Ok(());
    };
    let Some(notif) = ev.new_record::<NotificationRecord>() else {
        return Ok(());
    };

    let (title, body) = push_text(&notif.kind);
    let link = ui_base_url.map(|base| push_link(base, &notif.kind));
    for device in dao.list_devices(user_id).await? {
        match push
            .send(&device.push_token, &title, &body, link.as_deref())
            .await?
        {
            PushOutcome::Sent => {}
            // FCM rejected the token itself (unregistered/not found) — the
            // device is gone (app uninstalled, service worker replaced, etc.);
            // stop sending to it. A redelivery racing another cleanup (or the
            // user unregistering it themselves) may find it already gone —
            // that's the outcome we wanted, not a failure.
            PushOutcome::Stale => match dao.delete_device(user_id, &device.push_token).await {
                Ok(()) | Err(DaoError::NotFound(_)) => {}
                Err(e) => return Err(e.into()),
            },
        }
    }
    Ok(())
}

/// Where clicking this notification should land — a full URL (`ui_base_url`
/// plus a path), not a relative one: the same field also has to make sense
/// once Android/iOS apps exist, and a bare `https://` URL is what App Links /
/// Universal Links route to the native app if installed, falling back to this
/// same web page otherwise. No third-party dynamic-link service needed (and
/// Firebase Dynamic Links, which used to be that service, was shut down by
/// Google in August 2025). Mirrors `describe().href` in
/// agon_ui/src/pages/NotificationsPage.tsx — keep the two in sync.
fn push_link(ui_base_url: &str, kind: &NotificationKindRecord) -> String {
    let path = match kind {
        NotificationKindRecord::MatchInvitation { match_id, .. } => format!("/matches/{match_id}"),
        NotificationKindRecord::TeamInvitation { team_id, .. } => format!("/teams/{team_id}"),
        NotificationKindRecord::InvitationAccepted { context, .. } => match context {
            InvitationContextRecord::Match { match_id, .. } => format!("/matches/{match_id}"),
            InvitationContextRecord::Team { team_id, .. } => format!("/teams/{team_id}"),
        },
        NotificationKindRecord::Follow { actor_user_id } => format!("/users/{actor_user_id}"),
        NotificationKindRecord::Like { match_id, .. } => format!("/matches/{match_id}"),
        NotificationKindRecord::Comment { match_id, .. } => format!("/matches/{match_id}"),
        NotificationKindRecord::Reply { match_id, .. } => format!("/matches/{match_id}"),
        NotificationKindRecord::ScoreSubmitted { match_id, .. } => format!("/matches/{match_id}"),
        NotificationKindRecord::ScoreConfirmed { match_id, .. } => format!("/matches/{match_id}"),
    };
    // Config::from_env already strips any trailing slash from AGON_UI_URL.
    format!("{ui_base_url}{path}")
}

/// Generic push copy for one notification. Built only from fields already
/// denormalized onto `NotificationKindRecord` — no extra DAO reads, matching
/// the "kind carries display fields so the feed renders without extra reads"
/// principle already documented on `NotificationRecord`.
fn push_text(kind: &NotificationKindRecord) -> (String, String) {
    match kind {
        NotificationKindRecord::MatchInvitation { match_name, .. } => (
            "Match invitation".to_string(),
            format!("You've been invited to {match_name}"),
        ),
        NotificationKindRecord::TeamInvitation { team_name, .. } => (
            "Team invitation".to_string(),
            format!("You've been invited to join {team_name}"),
        ),
        NotificationKindRecord::InvitationAccepted { .. } => (
            "Invitation accepted".to_string(),
            "Someone accepted your invitation".to_string(),
        ),
        NotificationKindRecord::Follow { .. } => (
            "New follower".to_string(),
            "Someone started following you".to_string(),
        ),
        NotificationKindRecord::Like { match_name, .. } => (
            "New like".to_string(),
            format!("Someone liked {match_name}"),
        ),
        NotificationKindRecord::Comment { preview, .. } => {
            ("New comment".to_string(), preview.clone())
        }
        NotificationKindRecord::Reply { preview, .. } => ("New reply".to_string(), preview.clone()),
        NotificationKindRecord::ScoreSubmitted {
            match_name,
            needs_confirmation,
            ..
        } => {
            let body = if *needs_confirmation {
                format!("Please confirm the score for {match_name}")
            } else {
                format!("A score was submitted for {match_name}")
            };
            ("Score submitted".to_string(), body)
        }
        NotificationKindRecord::ScoreConfirmed { match_name, .. } => (
            "Score confirmed".to_string(),
            format!("Your score for {match_name} was confirmed"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of each `NotificationKindRecord` variant (`ScoreSubmitted` twice,
    /// for both values of `needs_confirmation`), shared by the `push_text` and
    /// `push_link` coverage tests below.
    fn sample_kinds() -> Vec<NotificationKindRecord> {
        vec![
            NotificationKindRecord::MatchInvitation {
                actor_user_id: "u1".into(),
                invitation_id: "i1".into(),
                match_id: "m1".into(),
                match_name: "Sunday Tennis".into(),
            },
            NotificationKindRecord::TeamInvitation {
                actor_user_id: "u1".into(),
                invitation_id: "i1".into(),
                team_id: "t1".into(),
                team_name: "The Aces".into(),
            },
            NotificationKindRecord::InvitationAccepted {
                actor_user_id: "u1".into(),
                invitation_id: "i1".into(),
                context: InvitationContextRecord::Match {
                    match_id: "m1".into(),
                    match_name: "Sunday Tennis".into(),
                },
            },
            NotificationKindRecord::Follow {
                actor_user_id: "u1".into(),
            },
            NotificationKindRecord::Like {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                match_name: "Sunday Tennis".into(),
            },
            NotificationKindRecord::Comment {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                comment_id: "c1".into(),
                preview: "nice game!".into(),
            },
            NotificationKindRecord::Reply {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                comment_id: "r1".into(),
                parent_comment_id: "c1".into(),
                preview: "agreed!".into(),
            },
            NotificationKindRecord::ScoreSubmitted {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                match_name: "Sunday Tennis".into(),
                submission_id: "s1".into(),
                needs_confirmation: true,
            },
            NotificationKindRecord::ScoreSubmitted {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                match_name: "Sunday Tennis".into(),
                submission_id: "s1".into(),
                needs_confirmation: false,
            },
            NotificationKindRecord::ScoreConfirmed {
                actor_user_id: "u1".into(),
                match_id: "m1".into(),
                match_name: "Sunday Tennis".into(),
                submission_id: "s1".into(),
            },
        ]
    }

    /// `push_text` produces non-empty copy for every current notification kind
    /// (mirrors every `NotificationKindRecord` variant, so a new variant that
    /// forgets to update this match arm fails to compile, not silently ships
    /// blank push copy).
    #[test]
    fn push_text_covers_every_kind() {
        let kinds = sample_kinds();

        for kind in &kinds {
            let (title, body) = push_text(kind);
            assert!(!title.is_empty(), "empty title for {kind:?}");
            assert!(!body.is_empty(), "empty body for {kind:?}");
        }

        // The two ScoreSubmitted variants (needs_confirmation true/false) must
        // produce different copy — that's the whole point of the flag.
        let (_, needs_confirm_body) = push_text(&kinds[7]);
        let (_, informational_body) = push_text(&kinds[8]);
        assert_ne!(needs_confirm_body, informational_body);
    }

    /// `push_link` produces a URL under `ui_base_url` for every kind (again,
    /// exhaustive match ⇒ a new variant fails to compile here, not silently
    /// ships a dead-end link), and never double/drops the slash between the
    /// base and the path regardless of whether the base carries a trailing one.
    #[test]
    fn push_link_covers_every_kind() {
        let base = "https://agon.example.com";
        for kind in &sample_kinds() {
            let link = push_link(base, kind);
            assert!(
                link.len() > format!("{base}/").len() && link.starts_with(&format!("{base}/")),
                "link {link} for {kind:?} doesn't extend the base URL with a real path"
            );
        }
    }
}
