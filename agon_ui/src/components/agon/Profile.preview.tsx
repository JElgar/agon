import type { components } from '@/types/api'
import { ProfileHeader } from './ProfileHeader'
import { SportStatsTable } from './SportStatsTable'
import { ThemeToggle } from '@/components/ThemeToggle'

type UserProfile = components['schemas']['UserProfile']

/**
 * Dev-only visual preview of the profile blocks. Reach it at `/?preview=profile`.
 * Not part of the app; safe to delete once the profile page is wired.
 */

const profile: UserProfile = {
  id: 'u-sofia',
  name: 'Sofia Lindqvist',
  stats: [
    { match_type: 'tennis', matches_played: 27, win_percentage: 66.7 },
    { match_type: 'football', matches_played: 19, win_percentage: 57.9 },
    { match_type: 'cricket', matches_played: 10, win_percentage: 40.0 },
    { match_type: 'squash', matches_played: 4, win_percentage: 25.0 },
  ],
  follower_count: 24,
  following_count: 31,
  is_followed_by_me: false,
}

const emptyProfile: UserProfile = {
  id: 'u-new',
  name: 'New Player',
  stats: [],
  follower_count: 0,
  following_count: 0,
  is_followed_by_me: false,
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-1.5 px-0.5 text-[10px] font-medium uppercase tracking-widest text-muted-foreground">
      {children}
    </div>
  )
}

export function ProfilePreview() {
  return (
    <div className="min-h-screen bg-background p-6">
      <div className="mx-auto flex max-w-md flex-col gap-6">
        <div className="flex items-center justify-between">
          <h1 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">
            Profile preview
          </h1>
          <ThemeToggle />
        </div>

        <ProfileHeader profile={profile} />

        <div>
          <SectionLabel>Sports</SectionLabel>
          <SportStatsTable stats={profile.stats} limit={3} />
        </div>

        <div>
          <SectionLabel>All sports</SectionLabel>
          <SportStatsTable stats={profile.stats} />
        </div>

        <div>
          <SectionLabel>New player (empty state)</SectionLabel>
          <ProfileHeader profile={emptyProfile} className="mb-4" />
          <SportStatsTable stats={emptyProfile.stats} />
        </div>
      </div>
    </div>
  )
}
