import type { components } from '@/types/api'

type JoinLinkPreview = components['schemas']['JoinLinkPreview']
type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

/** Which side(s) (if any) a join scope allows picking, and whether landing
 *  unassigned is offered — the client-side mirror of the server's
 *  `JoinScope`/`resolve_join_target` (see `agon_service/src/main.rs`). The
 *  server re-validates regardless; this only decides what a picker shows.
 *  Shared by `JoinMatchPage` (the join-link landing screen) and
 *  `JoinLinkBanner` (the same offer, resurfaced on the match detail page for
 *  a remembered link — see `lib/joinLinkMemory`). */
export interface JoinChoice {
  allowedSideIds: string[] | null
  allowUnassigned: boolean
}

export function joinChoiceFor(preview: JoinLinkPreview, match: Match): JoinChoice {
  const scope = preview.scope
  return {
    allowedSideIds: scope.side_ids ?? null,
    // The link's own preference, capped by the match's own ceiling — see
    // `Match.allow_unassigned`'s doc comment. The server re-enforces this
    // regardless; this only decides what the picker offers.
    allowUnassigned: scope.allow_unassigned && match.allow_unassigned,
  }
}

export function sidesFor(choice: JoinChoice, match: Match): MatchSide[] {
  if (choice.allowedSideIds === null) return match.sides
  const allowed = choice.allowedSideIds
  return match.sides.filter((s) => allowed.includes(s.id))
}
