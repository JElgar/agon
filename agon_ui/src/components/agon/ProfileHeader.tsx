import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Avatar } from './Avatar'
import { overallWinRate, totalMatches, formatWinRate } from '@/lib/stats'

type UserProfile = components['schemas']['UserProfile']

function Stat({ value, label }: { value: string | number; label: string }) {
  return (
    <div className="text-center">
      <div className="text-xl font-medium">{value}</div>
      <div className="mt-0.5 text-[10px] tracking-wide text-muted-foreground">
        {label}
      </div>
    </div>
  )
}

export interface ProfileHeaderProps
  extends React.HTMLAttributes<HTMLDivElement> {
  profile: UserProfile
}

/**
 * Profile header: avatar, name, and the follower / matches / win-rate stat row.
 * Matches count and overall win rate are derived from the profile's per-sport
 * `stats` (the win rate weighted by matches played, not a plain average).
 */
export function ProfileHeader({
  profile,
  className,
  ...props
}: ProfileHeaderProps) {
  const matches = totalMatches(profile.stats)
  const winRate = formatWinRate(overallWinRate(profile.stats))

  return (
    <div className={cn('flex flex-col gap-5', className)} {...props}>
      <div className="flex items-center gap-4">
        <Avatar
          name={profile.name}
          imageUrl={profile.profile_image?.image_url}
          size="xl"
          ring="you"
        />
        <div className="min-w-0">
          <h1 className="truncate text-xl font-medium">{profile.name}</h1>
        </div>
      </div>

      <div className="flex justify-around">
        <Stat value={profile.follower_count} label="Followers" />
        <Stat value={profile.following_count} label="Following" />
        <Stat value={matches} label="Matches" />
        <Stat value={winRate} label="Win rate" />
      </div>
    </div>
  )
}
