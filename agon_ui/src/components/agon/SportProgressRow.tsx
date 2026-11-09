import { Link } from 'react-router-dom'
import { ChevronRight } from 'lucide-react'
import type { MatchType } from '@/lib/sports'
import { sportIcon, sportLabel, sportTint } from '@/lib/sports'
import type { LadderPlacement, LadderRating } from '@/lib/rating'
import { formatWinRate } from '@/lib/stats'
import { cn } from '@/lib/utils'
import { PlacementsChip, RatingValue } from './RatingDisplay'

export interface SportProgressRowProps {
  sport: MatchType
  matchesPlayed: number
  winPercentage: number | null | undefined
  to: string
  isFirst?: boolean
  /** This profile's published rating on the sport's ladder, if it has one.
   *  Published ratings are public, so this is passed for anybody's profile. */
  rating?: LadderRating
  /** The viewer's own progress towards a rating on this ladder, when it isn't
   *  published yet. Never passed for someone else's profile — the API only
   *  gives a user their own placements, deliberately (see `lib/rating`'s
   *  `placementFor`), so the chip this draws can only ever be about yourself. */
  placement?: LadderPlacement
}

/**
 * One row of the profile's "Your sports" list: icon, the rating (or its
 * absence), matches and win rate, and a win-rate bar, linking to that sport's
 * stats page — the Profile board on the "Agon redesign" canvas
 * (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 *
 * The rating sits top-right, opposite the sport, because the three states it
 * has to hold are different widths and only one of them is a number: a
 * published rating with its `±`, a "still in placements" pill, or nothing at
 * all for a sport whose matches have all been friendlies. The stats line
 * moved below the sport name to make room, which is where the board puts it
 * for every row and not just the rated ones.
 */
export function SportProgressRow({
  sport,
  matchesPlayed,
  winPercentage,
  to,
  isFirst,
  rating,
  placement,
}: SportProgressRowProps) {
  const Icon = sportIcon(sport)
  const tint = sportTint(sport)
  const pct = Math.max(0, Math.min(100, winPercentage ?? 0))
  // Ranked games are worth naming separately from matches played: friendlies
  // move nothing, so "12 matches · 8 ranked" is what says how much of this
  // record the rating is actually built on.
  const ranked = rating?.matches_rated ?? placement?.played

  return (
    <Link
      to={to}
      className={cn(
        'flex min-h-16 items-center gap-3.5 px-4 py-3.5 text-foreground',
        !isFirst && 'border-t',
      )}
    >
      <span
        className="flex size-11 shrink-0 items-center justify-center rounded-full"
        style={{ background: tint.bg }}
      >
        <Icon className="size-[22px]" style={{ color: tint.fg }} strokeWidth={2} />
      </span>
      <span className="flex min-w-0 flex-grow flex-col gap-1.5">
        <span className="flex items-center gap-2">
          {/* Truncates because the right-hand side is the wider of the two:
              "Still in placements" beside "Table Tennis" is about all a
              390px row holds. */}
          <span className="truncate text-[16px] font-bold">{sportLabel(sport)}</span>
          <span className="flex-grow" />
          {rating ? <RatingValue rating={rating} /> : placement ? <PlacementsChip /> : null}
        </span>
        <span className="truncate text-[13px] text-muted-foreground">
          {matchesPlayed} matches
          {ranked ? ` · ${ranked} ranked` : ''} &middot; {formatWinRate(winPercentage)} won
        </span>
        <span className="h-1.5 overflow-hidden rounded-full bg-muted">
          <span
            className="block h-1.5 rounded-full bg-primary"
            style={{ width: `${pct}%` }}
          />
        </span>
      </span>
      <ChevronRight className="size-5 shrink-0 text-muted-foreground" />
    </Link>
  )
}
