import { useNavigate } from 'react-router-dom'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'

type TeamListItem = components['schemas']['TeamListItem']

export interface TeamCardProps {
  team: TeamListItem
}

/**
 * A single-line team row: logo (or initials), name, follower count. Clicking
 * anywhere opens the team's page. Mirrors `UserCard`'s layout so team and
 * user lists read as one visual system.
 */
export function TeamCard({ team }: TeamCardProps) {
  const navigate = useNavigate()

  return (
    <button
      type="button"
      onClick={() => navigate(`/teams/${team.id}`)}
      className="flex w-full items-center gap-3 px-4 py-3 text-left"
    >
      <Avatar name={team.name} imageUrl={team.logo?.image_url} size="lg" />
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{team.name}</div>
        <div className="truncate text-xs text-muted-foreground">
          {team.follower_count.toLocaleString()}{' '}
          {team.follower_count === 1 ? 'follower' : 'followers'}
        </div>
      </div>
    </button>
  )
}
