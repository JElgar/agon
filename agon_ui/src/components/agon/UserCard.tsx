import { useNavigate } from 'react-router-dom'
import type { components } from '@/types/api'
import { Avatar } from './Avatar'
import { FollowButton } from './FollowButton'
import { totalMatches } from '@/lib/stats'

type UserProfile = components['schemas']['UserProfile']

export interface UserCardProps {
  user: UserProfile
  /** The viewer's own id; when it matches `user.id` the follow button is hidden. */
  currentUserId?: string
}

/**
 * A single-line user row: avatar, name, a follower / matches summary, and a
 * follow toggle. Clicking the row (anywhere but the button) opens the user's
 * profile. Shared by the search page and the follower/following lists, so all
 * three render identically and stay in sync via the shared `FollowButton`.
 */
export function UserCard({ user, currentUserId }: UserCardProps) {
  const navigate = useNavigate()
  const isSelf = currentUserId === user.id
  const matches = totalMatches(user.stats)

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <button
        type="button"
        onClick={() => navigate(`/users/${user.id}`)}
        className="flex min-w-0 flex-1 items-center gap-3 text-left"
      >
        <Avatar
          name={user.name}
          imageUrl={user.profile_image?.image_url}
          size="lg"
        />
        <div className="min-w-0">
          <div className="truncate text-sm font-medium">{user.name}</div>
          <div className="truncate text-xs text-muted-foreground">
            {user.follower_count.toLocaleString()}{' '}
            {user.follower_count === 1 ? 'follower' : 'followers'} · {matches}{' '}
            {matches === 1 ? 'match' : 'matches'}
          </div>
        </div>
      </button>

      {!isSelf && (
        <FollowButton
          userId={user.id}
          isFollowing={user.is_followed_by_me}
          size="sm"
          className="shrink-0"
        />
      )}
    </div>
  )
}
