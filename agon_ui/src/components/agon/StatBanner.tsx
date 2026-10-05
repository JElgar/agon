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

/** Cycling tints for the per-sport breakdown bar/legend, on the soft-tint
 *  banner — matches the canvas's blue/terracotta two-sport example (`--link`
 *  reads correctly as a fill on both the light tint and the dark navy
 *  surface), extended so a third+ sport still fades the foreground color
 *  rather than inventing an undesigned hue. */
const SEGMENT_TINTS = [
  'var(--link)',
  'var(--banner-blue-accent)',
  'color-mix(in oklch, var(--foreground) 55%, transparent)',
  'color-mix(in oklch, var(--foreground) 30%, transparent)',
]

export interface StatBannerProps {
  name: string
  profileImageUrl?: string
  stats: UserStats
}

/**
 * The soft-tint "your season so far" hero banner (feed/home) — greeting,
 * the matches/wins/win-rate headline, and a segmented bar breaking those
 * matches down by sport. See the "Agon redesign" canvas's `LightMain.dc.html`.
 */
export function StatBanner({ name, profileImageUrl, stats }: StatBannerProps) {
  const played = totalMatches(stats)
  const wins = totalWins(stats)
  const winRate = overallWinRate(stats)
  const bySport = sortedByActivity(stats).filter((s) => s.stats.matches_played > 0)

  return (
    <Card className="border-banner-tint-border bg-banner-tint text-foreground">
      <CardContent className="flex flex-col gap-4">
        <div className="flex items-center gap-3">
          <Avatar name={name} imageUrl={profileImageUrl} size="lg" className="bg-white text-primary" />
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <p className="font-semibold">Morning, {name.split(' ')[0]}</p>
            <p className="text-sm text-banner-tint-muted-foreground">Your season so far</p>
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
          <StatTile value={played} label="Matches played" tone="banner-blue" />
          <StatTile value={wins} label="Wins" tone="banner-blue" />
          <StatTile value={formatWinRate(winRate)} label="Win rate" tone="banner-blue" />
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
            <div className="flex flex-wrap gap-4 text-sm text-banner-tint-muted-foreground">
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
