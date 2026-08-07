import type { components } from '@/types/api'

type Member = components['schemas']['Member']
type MatchPlayer = components['schemas']['MatchPlayer']
type MatchSide = components['schemas']['MatchSide']
type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']
type Invitation = components['schemas']['Invitation']
/** Name/avatar only, not the full `Member` shape — a side's `roster_preview`
 *  entry, or a resolved id in a cricket/football score's `players` map (see
 *  `CricketScore.players`'s backend doc comment). */
export type RosterPreviewPlayer = components['schemas']['RosterPreviewPlayer']
/** A cricket or football score's resolved-name lookup (`CricketScore.players`
 *  / `FootballScore.players`), keyed by match-scoped player id — what
 *  `playerNameFor` (both sports' versions) checks before falling back to
 *  scanning the match's full roster, which a feed/search card doesn't carry
 *  at all. */
export type ScorePlayers = Record<string, RosterPreviewPlayer>
// The generated `invitation.kind` type erases the discriminant (`Omit<…,"type">
// & unknown`), so `.type` won't narrow. Use the real union for token extraction.
type InvitationKind = components['schemas']['InvitationKind']

/**
 * Anything with an optional `players` list: a full `Match`, a locally-built
 * draft (e.g. `FootballScoreFields`' not-yet-created match), or one of the
 * trimmed shapes with no `players` field at all — a feed's `FeedMatch` (see
 * `known_participants`/`viewer_side_id`) or search/profile-activity's
 * `SearchMatch`.
 *
 * The `FeedMatch`/`SearchMatch` branches are listed explicitly (rather than
 * relying on `{ players?: ... }` alone) because TypeScript's weak-type check
 * rejects such an argument against an all-optional object type on its own:
 * it shares literally no properties with `{ players?: ... }`. Narrow with
 * `'players' in match`.
 */
type MatchLike = { players?: MatchPlayer[] } | FeedMatch | SearchMatch

/**
 * The bearer invite token for a member with a pending token-invitation, else
 * null. Only token-kind invitations (external people invited by name) carry a
 * shareable link; user-kind invites are accepted in-app by the target account.
 */
export function memberInviteToken(member: Member): string | null {
  const invitation = member.invitation
  if (!invitation || invitation.status !== 'pending') return null
  const kind = invitation.kind as InvitationKind
  return kind.type === 'Token' ? kind.invite_token : null
}

/** Absolute invite-link URL for a token, matching the `/invite/:token` route. */
export function inviteLink(token: string): string {
  return `${window.location.origin}/invite/${encodeURIComponent(token)}`
}

/**
 * The viewer's own pending invitation to a match, if they've been invited (as a
 * known Agon user) and haven't yet responded — else null. Lets match views show
 * the viewer their invite and an accept/decline action, mirroring the inbox.
 * Only user-kind invites apply: the viewer is a signed-in account, matched by id.
 *
 * Also accepts a feed's `FeedMatch`, which never carries the full roster —
 * always `null` there, which is correct: an invitee has no feed row for a
 * match until they accept (see the server's fan-out doc comments), so a feed
 * card never has a pending invitation to show in the first place.
 */
export function myPendingInvitation(
  match: MatchLike,
  currentUserId: string | undefined,
): Invitation | null {
  if (!currentUserId) return null
  const players = ('players' in match && match.players) || []
  for (const player of players) {
    if (player.member.type !== 'User') continue
    if (player.member.user_id !== currentUserId) continue
    const invitation = player.member.invitation
    if (invitation && invitation.status === 'pending') return invitation
  }
  return null
}

/**
 * Return a copy of `match` with the given user's invitation marked with a new
 * status — used to optimistically reflect an accept/decline before the server
 * confirms, so the invite banner/badge update the instant the user clicks
 * (mirroring the feed's immediate-update behaviour). Leaves the match untouched
 * if the user has no matching player. Immutable: builds new player objects so
 * react-query change-detection re-renders consumers.
 */
export function withInvitationStatus(
  match: Match,
  userId: string,
  status: Invitation['status'],
): Match {
  return {
    ...match,
    players: match.players.map((player) => {
      if (player.member.type !== 'User') return player
      if (player.member.user_id !== userId) return player
      if (!player.member.invitation) return player
      return {
        ...player,
        member: {
          ...player.member,
          invitation: { ...player.member.invitation, status },
        },
      }
    }),
  }
}

/**
 * Whether the viewer is a participant in the match — a linked player who was
 * either added ad-hoc (no invitation) or has accepted. Mirrors the server's
 * `caller_is_participant`: participants may edit the match, invite others, and
 * record the result. Pending/declined invitees are not participants.
 *
 * Also accepts a feed's `FeedMatch` (see `myPendingInvitation`'s doc comment)
 * — always `false` there; check `FeedMatch.viewer_side_id` instead if you
 * need "is the viewer playing" from a feed card.
 */
export function isParticipant(
  match: MatchLike,
  currentUserId: string | undefined,
): boolean {
  if (!currentUserId) return false
  const players = ('players' in match && match.players) || []
  return players.some((player) => {
    if (player.member.type !== 'User') return false
    if (player.member.user_id !== currentUserId) return false
    const invitation = player.member.invitation
    return !invitation || invitation.status === 'accepted'
  })
}

/**
 * Display name for a match player: a linked Agon user's name is hydrated onto
 * the member server-side, an external player carries a display name directly.
 */
export function memberName(member: Member): string {
  return member.type === 'External' ? member.display_name : member.name
}

/** Avatar image for a match player, if the linked Agon user has one set. */
export function memberAvatarUrl(member: Member): string | undefined {
  return member.type === 'User' ? member.avatar_url : undefined
}

/** Initials for an avatar, from a display name (e.g. "Sofia Lindqvist" → "SL"). */
export function initials(name: string | undefined | null): string {
  // Defensive: a missing name (e.g. a not-yet-hydrated profile) yields a neutral
  // placeholder rather than throwing — an avatar should never crash its page.
  const parts = (name ?? '').trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

/** The players assigned to a given side. */
export function playersOnSide(
  players: MatchPlayer[],
  side: MatchSide,
): MatchPlayer[] {
  return players.filter((p) => p.side_id === side.id)
}

/** The stable member id for a player, used to key rows and match score events. */
export function playerId(player: MatchPlayer): string {
  return player.member.id
}
