import { Info } from 'lucide-react'
import { Card } from '@/components/ui/card'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { sportLabel, type MatchType } from '@/lib/sports'
import {
  canBeRanked,
  placementsRemainingLine,
  type LadderPlacement,
  type LadderRating,
} from '@/lib/rating'
import { PlacementsBar, RatingValue } from './RatingDisplay'

export interface RatingCardProps {
  sport: MatchType
  /** The subject's published rating on this ladder, if they have one. */
  rating?: LadderRating
  /** The viewer's own progress, when the rating isn't published yet. Only
   *  ever available for your own profile. */
  placement?: LadderPlacement
  /** Whether this page is the viewer's own profile. Drives what the empty
   *  state is allowed to say. */
  isOwnProfile: boolean
}

/**
 * A sport's rating, on its stats page — the rating card from the EloSport
 * board on the "Agon redesign" canvas
 * (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 *
 * The board draws this as "Rating over time": a line with a confidence band
 * around it. That chart isn't here, and can't be yet — no endpoint returns a
 * rating's history. The per-match contributions exist in the table
 * (`RATINGCONTRIB#` items) but nothing exposes them, so the series behind the
 * chart has no source. What's left is the part the API does answer: the
 * current number, its ±, and how many ranked games are behind it. Hence
 * "Rating" rather than "Rating over time" — the board's own title would be
 * promising a history this card hasn't got.
 *
 * Returns nothing rather than an empty state when there's nothing it may say:
 * a sport with no ladder at all (`other`), or somebody else's sport with no
 * published rating — where a "no rating yet" line would be guessing between
 * placements and friendlies, and saying either would be telling the viewer
 * something the API deliberately withholds.
 */
export function RatingCard({ sport, rating, placement, isOwnProfile }: RatingCardProps) {
  if (!canBeRanked(sport)) return null
  if (!rating && !placement && !isOwnProfile) return null

  const ladder = sportLabel(sport).toLowerCase()
  // A ladder is in exactly one of `ratings` and `rating_placements`, never
  // both — so these two never disagree about which count to show.
  const ranked = rating?.matches_rated ?? placement?.played

  return (
    <Card className="flex flex-col gap-3.5 p-[18px]">
      <div className="flex items-center justify-between gap-2">
        <span className="flex items-center gap-1">
          <span className="text-[15px] font-bold">
            {placement && !rating ? 'Still in placements' : 'Rating'}
          </span>
          <HowRatingsWork />
        </span>
        {ranked !== undefined && (
          <span className="text-[13px] text-muted-foreground">
            {ranked} ranked {ranked === 1 ? 'match' : 'matches'}
          </span>
        )}
      </div>

      {rating ? (
        <RatingValue rating={rating} size="hero" className="self-start" />
      ) : placement ? (
        <div className="flex flex-col gap-2.5">
          <span className="flex items-baseline gap-2">
            <span className="font-display text-[34px] leading-none font-extrabold tabular-nums">
              {placement.played} of {placement.required}
            </span>
            <span className="text-sm font-semibold text-muted-foreground">placements played</span>
          </span>
          <PlacementsBar played={placement.played} required={placement.required} size="lg" />
          <span className="text-[13px] text-muted-foreground">
            {placementsRemainingLine(placement, sport)}
          </span>
        </div>
      ) : (
        <span className="text-[13px] text-muted-foreground">
          No rating yet. Play a ranked {ladder} match to start one.
        </span>
      )}
    </Card>
  )
}

/**
 * The "i" beside the heading, and the two things somebody looking at a
 * rating for the first time needs told: that nobody opted into this, and
 * that the first five games are withheld. Wording from the board's own
 * dialog.
 */
function HowRatingsWork() {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label="How ratings work"
          className="inline-flex size-7 items-center justify-center rounded-full text-muted-foreground transition-colors hover:text-foreground"
        >
          <Info className="size-[18px]" />
        </button>
      </PopoverTrigger>
      <PopoverContent side="bottom" align="start" className="flex w-[19rem] flex-col gap-3 p-4">
        <span className="text-[15px] font-bold text-foreground">How ratings work</span>
        <Point n={1}>
          A match is ranked or friendly from the moment it is created. Playing a ranked one is the
          whole agreement — there is nothing to opt into, and a friendly never moves a rating.
        </Point>
        <Point n={2}>
          Your first five ranked games in a sport are placements. Nobody sees the number until they
          are done, you included — early on it swings far too much to mean anything.
        </Point>
      </PopoverContent>
    </Popover>
  )
}

function Point({ n, children }: { n: number; children: React.ReactNode }) {
  return (
    <span className="flex items-start gap-2.5">
      <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-accent text-[11px] font-extrabold text-accent-foreground">
        {n}
      </span>
      <span className="text-[13px] leading-snug text-ink-soft">{children}</span>
    </span>
  )
}
