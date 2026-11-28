//! Team settings against DynamoDB Local: a team's `sports` and
//! `members_join_default` round-trip through create and update, and a
//! pending team invitation can be re-sent (`Dao::resend_team_invitation`)
//! with its bookkeeping (`invited_at`, `resend_count`, inbox sort key, the
//! embedded copy on the member row) kept in step, and every send's
//! notification marked actioned once the invitee responds.
//!
//! Skipped unless `AWS_ENDPOINT_URL` is set (see `common::local_env`), so it
//! never touches a real table:
//!
//! ```text
//! docker compose up -d dynamodb-local dynamodb-local-init
//! AWS_ENDPOINT_URL=http://localhost:8002 AWS_REGION=eu-west-1 AWS_ACCESS_KEY_ID=local \
//!   AWS_SECRET_ACCESS_KEY=localsecret cargo test -p agon_services_core --test team_settings_roundtrip
//! ```

mod common;

use agon_services_core::dao::DaoError;
use agon_services_core::dao::records::{
    EmbeddedInvitationRecord, InvitationContextRecord, InvitationKindRecord, InvitationRecord,
    NotificationKindRecord, NotificationRecord, TeamMemberRecord, TeamRecord,
    invitation_notification_id,
};
use common::{TestEnv, local_env};

const T0: &str = "2026-11-20T10:00:00.000Z";
const T1: &str = "2026-11-22T10:00:00.000Z";

fn team(id: &str) -> TeamRecord {
    TeamRecord {
        id: id.into(),
        name: "Reds".into(),
        logo_url: None,
        invite_token: None,
        follower_count: 0,
        created_at: T0.into(),
        colour: None,
        tournament_id: None,
        sports: vec!["cricket".into(), "football".into()],
        members_join_default: true,
    }
}

fn owner(team_id: &str, user_id: &str) -> TeamMemberRecord {
    TeamMemberRecord {
        team_id: team_id.into(),
        membership_id: format!("{team_id}-owner"),
        user_id: Some(user_id.into()),
        display_name: None,
        role: "owner".into(),
        invitation: None,
        created_at: T0.into(),
    }
}

#[tokio::test]
async fn team_settings_round_trip() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let team_id = format!("tset_{run}");
    dao.create_team(&team(&team_id), &owner(&team_id, &format!("u_{run}")))
        .await
        .unwrap();

    let meta = dao.get_team_meta(&team_id).await.unwrap().unwrap();
    assert_eq!(meta.sports, vec!["cricket", "football"]);
    assert!(meta.members_join_default);

    // Untouched by an update that names neither.
    dao.update_team(&team_id, Some("Crimson"), None, None, None)
        .await
        .unwrap();
    let meta = dao.get_team_meta(&team_id).await.unwrap().unwrap();
    assert_eq!(meta.name, "Crimson");
    assert_eq!(meta.sports, vec!["cricket", "football"]);
    assert!(meta.members_join_default);

    // Replaced (order kept) and turned off.
    let sports = vec!["football".to_string(), "netball".to_string()];
    dao.update_team(&team_id, None, None, Some(&sports), Some(false))
        .await
        .unwrap();
    let meta = dao.get_team_meta(&team_id).await.unwrap().unwrap();
    assert_eq!(meta.sports, sports);
    assert!(!meta.members_join_default);

    // Cleared: read back as an empty list, the same as a team that never set
    // one (an older team, with neither attribute stored).
    dao.update_team(&team_id, None, None, Some(&[]), None)
        .await
        .unwrap();
    let meta = dao.get_team_meta(&team_id).await.unwrap().unwrap();
    assert!(meta.sports.is_empty());

    let legacy_id = format!("tset_legacy_{run}");
    let legacy = TeamRecord {
        sports: Vec::new(),
        members_join_default: false,
        ..team(&legacy_id)
    };
    dao.create_team(&legacy, &owner(&legacy_id, &format!("u_{run}")))
        .await
        .unwrap();
    let meta = dao.get_team_meta(&legacy_id).await.unwrap().unwrap();
    assert!(meta.sports.is_empty());
    assert!(!meta.members_join_default);

    assert!(matches!(
        dao.update_team(&format!("nope_{run}"), None, None, None, Some(true))
            .await,
        Err(DaoError::NotFound(_))
    ));
}

#[tokio::test]
async fn resend_team_invitation_bookkeeping() {
    let Some(TestEnv { dao, .. }) = local_env().await else {
        eprintln!("AWS_ENDPOINT_URL unset; skipping");
        return;
    };
    let run = rand::random::<u32>();
    let team_id = format!("tresend_{run}");
    let host = format!("host_{run}");
    let guest = format!("guest_{run}");
    let invitation_id = format!("inv_{run}");
    let membership_id = format!("{team_id}-guest");
    dao.create_team(&team(&team_id), &owner(&team_id, &host))
        .await
        .unwrap();

    let kind = InvitationKindRecord::User {
        invited_user_id: guest.clone(),
    };
    dao.put_team_member(
        &team_id,
        &TeamMemberRecord {
            team_id: team_id.clone(),
            membership_id: membership_id.clone(),
            user_id: Some(guest.clone()),
            display_name: None,
            role: "member".into(),
            invitation: Some(EmbeddedInvitationRecord {
                id: invitation_id.clone(),
                status: "pending".into(),
                invited_by_user_id: host.clone(),
                invited_at: T0.into(),
                responded_at: None,
                kind: kind.clone(),
                reserve_spot: false,
            }),
            created_at: T0.into(),
        },
    )
    .await
    .unwrap();
    let inv = InvitationRecord {
        id: invitation_id.clone(),
        status: "pending".into(),
        invited_by_user_id: host.clone(),
        invited_user_id: Some(guest.clone()),
        invite_token: None,
        kind,
        context: InvitationContextRecord::Team {
            team_id: team_id.clone(),
            team_name: "Reds".into(),
        },
        invited_at: T0.into(),
        responded_at: None,
        reserve_spot: false,
        resend_count: 0,
    };
    dao.create_invitation(&inv).await.unwrap();

    dao.resend_team_invitation(&team_id, &membership_id, &invitation_id, T0, T1, true)
        .await
        .unwrap();

    let stored = dao.get_invitation(&invitation_id).await.unwrap().unwrap();
    assert_eq!(stored.invited_at, T1);
    assert_eq!(stored.resend_count, 1);
    assert_eq!(stored.status, "pending");
    let agg = dao.get_team(&team_id).await.unwrap().unwrap();
    let member = agg
        .members
        .iter()
        .find(|m| m.membership_id == membership_id)
        .unwrap();
    assert_eq!(member.invitation.as_ref().unwrap().invited_at, T1);
    // The inbox's pending filter still finds it (GSI1SK re-stamped).
    let inbox = dao
        .list_user_invitations(&guest, Some("pending"), None, 20)
        .await
        .unwrap();
    assert_eq!(inbox.items.len(), 1);
    assert_eq!(inbox.items[0].invited_at, T1);

    // A second re-send racing on the old `invited_at` loses.
    assert!(matches!(
        dao.resend_team_invitation(&team_id, &membership_id, &invitation_id, T0, T1, true)
            .await,
        Err(DaoError::Conflict(_))
    ));

    // The worker writes one notification per send; responding marks them
    // all actioned.
    for n in 0..=stored.resend_count {
        dao.create_notification(&NotificationRecord {
            id: invitation_notification_id(&invitation_id, n),
            user_id: guest.clone(),
            is_read: false,
            created_at: T0.into(),
            kind: NotificationKindRecord::TeamInvitation {
                actor_user_id: host.clone(),
                invitation_id: invitation_id.clone(),
                team_id: team_id.clone(),
                team_name: "Reds".into(),
                status: "pending".into(),
            },
        })
        .await
        .unwrap();
    }
    dao.respond_to_invitation(&invitation_id, "declined", T1, T1, true)
        .await
        .unwrap();
    dao.mark_invitation_notification_actioned(
        &guest,
        &invitation_id,
        stored.resend_count,
        "declined",
    )
    .await
    .unwrap();
    let notifs = dao.list_notifications(&guest, None, 20).await.unwrap();
    assert_eq!(notifs.items.len(), 2);
    for n in &notifs.items {
        match &n.kind {
            NotificationKindRecord::TeamInvitation { status, .. } => {
                assert_eq!(status, "declined", "{}", n.id)
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    // Once answered, it can't be re-sent.
    assert!(matches!(
        dao.resend_team_invitation(&team_id, &membership_id, &invitation_id, T1, T1, true)
            .await,
        Err(DaoError::Conflict(_))
    ));
}
