use poem_openapi::{Object, Union};

use crate::UserProfile;
use crate::membership::InvitationContext;

/// A notification feed entry. A thin, read-only record of "something happened"
/// that references the underlying entity rather than owning its state — the
/// action buttons resolve to the existing domain endpoints (e.g. a match
/// invitation notification carries an `invitation_id` and Confirm calls
/// `POST /invitations/:id/respond`). Notifications never store actionable state
/// such as invitation status, so they cannot drift out of sync with it.
#[derive(Object)]
pub struct Notification {
    pub id: String,
    pub is_read: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// The event. Person-triggered kinds carry an `actor`; system-generated
    /// kinds (e.g. a match completing because time passed) simply omit it.
    pub kind: NotificationKind,
}

/// The kind of notification. Each variant carries only the references and small
/// display hints (labels, previews) needed to render the row and wire its action
/// without extra fetches. Display hints are snapshots; live state is read from
/// the referenced entity.
#[derive(Union)]
#[oai(one_of, discriminator_name = "type")]
pub enum NotificationKind {
    /// You were invited to / tagged in a match. Confirm/Decline act on the
    /// referenced invitation.
    MatchInvitation(MatchInvitationNotification),
    /// You were invited to a team. Confirm/Decline act on the referenced
    /// invitation.
    TeamInvitation(TeamInvitationNotification),
    /// An invitation you sent (to a match or team) was accepted.
    InvitationAccepted(InvitationAcceptedNotification),
    /// Someone followed you.
    Follow(FollowNotification),
    /// Someone liked your match.
    Like(LikeNotification),
    /// Someone commented on a match.
    Comment(CommentNotification),
    /// Someone replied to a comment (to yours, or on a match you played in).
    Reply(ReplyNotification),
    /// A score was submitted for a match you played in. `needs_confirmation`
    /// tells the client whether to render the confirm/dispute action (your side
    /// must respond) or a plain informational row.
    ScoreSubmitted(ScoreSubmittedNotification),
    /// A score you submitted was confirmed by the other side(s).
    ScoreConfirmed(ScoreConfirmedNotification),
}

#[derive(Object)]
pub struct MatchInvitationNotification {
    /// The user who sent the invite.
    pub inviter: UserProfile,
    /// The invitation to act on (Confirm/Decline → the invitation endpoints).
    pub invitation_id: String,
    pub match_id: String,
    /// Display label so the row renders without fetching the match.
    pub match_name: String,
}

#[derive(Object)]
pub struct TeamInvitationNotification {
    /// The user who sent the invite.
    pub inviter: UserProfile,
    /// The invitation to act on (Confirm/Decline → the invitation endpoints).
    pub invitation_id: String,
    pub team_id: String,
    /// Display label so the row renders without fetching the team.
    pub team_name: String,
}

#[derive(Object)]
pub struct InvitationAcceptedNotification {
    /// The user who accepted.
    pub accepted_by: UserProfile,
    /// The invitation that was accepted.
    pub invitation_id: String,
    /// What the accepted invitation was to (a match or a team).
    pub context: InvitationContext,
}

#[derive(Object)]
pub struct FollowNotification {
    /// The user who followed you. Lets the client offer a follow-back action.
    pub follower: UserProfile,
}

#[derive(Object)]
pub struct LikeNotification {
    /// The user who liked the match.
    pub liked_by: UserProfile,
    pub match_id: String,
    pub match_name: String,
}

#[derive(Object)]
pub struct CommentNotification {
    /// The user who commented.
    pub commenter: UserProfile,
    pub match_id: String,
    pub comment_id: String,
    /// A short snapshot of the comment text for the row.
    pub preview: String,
}

#[derive(Object)]
pub struct ReplyNotification {
    /// The user who replied.
    pub replier: UserProfile,
    pub match_id: String,
    /// The reply's own comment id.
    pub comment_id: String,
    /// The top-level comment whose thread the reply belongs to (so the client
    /// can open the thread).
    pub parent_comment_id: String,
    /// A short snapshot of the reply text for the row.
    pub preview: String,
}

#[derive(Object)]
pub struct ScoreSubmittedNotification {
    /// The user who submitted the score.
    pub submitted_by: UserProfile,
    pub match_id: String,
    /// Display label so the row renders without fetching the match.
    pub match_name: String,
    /// The submission to act on (Confirm/Dispute → the score-submission
    /// respond endpoint) when `needs_confirmation` is true.
    pub submission_id: String,
    /// True => the recipient's side still needs to confirm/dispute; the client
    /// should render the action. False => informational only.
    pub needs_confirmation: bool,
}

#[derive(Object)]
pub struct ScoreConfirmedNotification {
    /// The participant whose confirmation completed the score.
    pub confirmed_by: UserProfile,
    pub match_id: String,
    /// Display label so the row renders without fetching the match.
    pub match_name: String,
    pub submission_id: String,
}

/// One page of notifications. `next_cursor` absent => end.
#[derive(Object)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    pub next_cursor: Option<String>,
}

/// The unread count for the bell badge.
#[derive(Object)]
pub struct UnreadCount {
    pub unread_count: u32,
}
