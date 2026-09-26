import { Link } from 'react-router-dom'
import { ChevronRight } from 'lucide-react'
import { Card, CardContent } from '@/components/ui/card'
import { Avatar } from './Avatar'
import { StatTile } from './StatTile'
import {
  formatWinRate,
  overallWinRate,
  sortedByActivity,
  totalMatches,
  totalWins,
  type UserStats,
} from '@/lib/stats'
import { sportLabel } from '@/lib/sports'

/** Cycling tints for the per-sport breakdown bar/legend, on top of the
 *  primary-blue banner — matches the canvas's white/peach two-sport example,
 *  extended so a third+ sport still reads clearly. */
const SEGMENT_TINTS = ['#FFFFFF', '#FFC9A8', '#BFD0FF', '#D7F5C9']

export interface StatBannerProps {
  name: string
  profileImageUrl?: string
  stats: UserStats
}

/**
 * The blue "your season so far" hero banner (feed/home) — greeting, the
 * matches/wins/win-rate headline, and a segmented bar breaking those matches
 * down by sport. See the "Agon redesign" canvas's `Main.dc.html`.
 */
export function StatBanner({ name, profileImageUrl, stats }: StatBannerProps) {
  const played = totalMatches(stats)
  const wins = totalWins(stats)
  const winRate = overallWinRate(stats)
  const bySport = sortedByActivity(stats).filter((s) => s.stats.matches_played > 0)

  return (
    <Card className="border-primary bg-primary text-primary-foreground">
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center gap-3">
          <Avatar name={name} imageUrl={profileImageUrl} size="lg" className="bg-white text-primary" />
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <p className="font-semibold">Morning, {name.split(' ')[0]}</p>
            <p className="text-sm text-primary-foreground/80">Your season so far</p>
          </div>
          <Link
            to="/profile"
            aria-label="See your profile"
            className="flex size-11 shrink-0 items-center justify-center rounded-full"
          >
            <ChevronRight className="size-5" />
          </Link>
        </div>

        <div className="grid grid-cols-3 gap-2">
          <StatTile value={played} label="Matches played" tone="inverted" />
          <StatTile value={wins} label="Wins" tone="inverted" />
          <StatTile value={formatWinRate(winRate)} label="Win rate" tone="inverted" />
        </div>

        {bySport.length > 1 && (
          <div className="flex flex-col gap-2">
            <div className="flex h-2 gap-0.5 overflow-hidden rounded-full">
              {bySport.map((s, i) => (
                <span
                  key={s.sport}
                  className="rounded-full"
                  style={{
                    width: `${(s.stats.matches_played / played) * 100}%`,
                    backgroundColor: SEGMENT_TINTS[i % SEGMENT_TINTS.length],
                  }}
                />
              ))}
            </div>
            <div className="flex flex-wrap gap-4 text-sm text-primary-foreground/90">
              {bySport.map((s, i) => (
                <span key={s.sport} className="flex items-center gap-1.5">
                  <span
                    className="size-2 rounded-full"
                    style={{ backgroundColor: SEGMENT_TINTS[i % SEGMENT_TINTS.length] }}
                  />
                  {sportLabel(s.sport)} {s.stats.matches_played}
                </span>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
