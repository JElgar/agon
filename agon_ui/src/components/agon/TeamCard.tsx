import { useNavigate } from 'react-router-dom'
import { ChevronRight } from 'lucide-react'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'

type TeamListItem = components['schemas']['TeamListItem']

export interface TeamCardProps {
  team: TeamListItem
}

/**
 * A single-line team row: crest (or initials), name, a follower-count
 * summary, and a chevron. Clicking anywhere opens the team's page. Matches
 * the redesign's Teams list row (crest as a rounded-square badge) — see the
 * "Agon redesign" canvas (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc,
 * `project/Teams.dc.html`).
 *
 * The mock's row also shows the viewer's role and a member count
 * ("Owner · 12 members · 61 followers"), but `TeamListItem` (the `GET
 * /users/me/teams` list shape) carries neither — only `follower_count`.
 * Fetching either per-row would mean an extra request per team in the list,
 * so this shows the follower count alone rather than inventing a backend
 * field or an N+1 fetch; see the PR description for this known gap.
 */
export function TeamCard({ team }: TeamCardProps) {
  const navigate = useNavigate()

  return (
    <button
      type="button"
      onClick={() => navigate(`/teams/${team.id}`)}
      className="flex w-full items-center gap-3.5 px-4 py-3.5 text-left transition-colors hover:bg-accent/40"
    >
      <Avatar
        name={team.name}
        imageUrl={team.logo?.image_url}
        size="lg"
        className="size-12 rounded-2xl text-base"
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-base font-bold">{team.name}</div>
        <div className="truncate text-sm text-muted-foreground">
          {team.follower_count.toLocaleString()}{' '}
          {team.follower_count === 1 ? 'follower' : 'followers'}
        </div>
      </div>
      <ChevronRight className="size-5 shrink-0 text-muted-foreground" />
    </button>
  )
}
