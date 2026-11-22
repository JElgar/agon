import { PLACEMENTS_LABEL, confidenceLabel, ratingAriaLabel, type LadderRating } from '@/lib/rating'
import { cn } from '@/lib/utils'

/**
 * A published rating and its `±`, e.g. "1642 ±704" — the pair, never the
 * number alone.
 *
 * The `±` isn't decoration: five ranked games leave a new player's confidence
 * around ±650 and a five-a-side player's nearer ±740, so the number beside it
 * is a far looser claim than a bare "1642" looks. `row` is the profile's
 * sports list, `hero` the headline on a sport's own page.
 */
export function RatingValue({
  rating,
  size = 'row',
  className,
}: {
  rating: LadderRating
  size?: 'row' | 'hero'
  className?: string
}) {
  return (
    <span
      className={cn('flex shrink-0 items-baseline gap-1.5', className)}
      aria-label={ratingAriaLabel(rating)}
    >
      <span
        aria-hidden
        className={cn(
          'font-display font-extrabold leading-none tabular-nums',
          size === 'hero' ? 'text-[34px]' : 'text-[22px]',
        )}
      >
        {rating.rating}
      </span>
      <span
        aria-hidden
        className={cn(
          'font-semibold text-muted-foreground',
          size === 'hero' ? 'text-sm' : 'text-[13px]',
        )}
      >
        {confidenceLabel(rating.confidence)}
      </span>
    </span>
  )
}

/**
 * The pill that stands where a rating would go while it's still being
 * withheld. See `PLACEMENTS_LABEL` for why it carries no count.
 *
 * Only ever rendered for the viewer's own ladders — nothing tells us anybody
 * else is mid-placements, and that's by design.
 */
export function PlacementsChip({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        'inline-flex h-6 shrink-0 items-center rounded-full bg-chip px-2.5 text-xs font-bold text-ink-soft',
        className,
      )}
    >
      {PLACEMENTS_LABEL}
    </span>
  )
}

/**
 * Placement progress as one segment per required game: played ones solid,
 * the rest empty. `pending` tints the next segment instead of leaving it
 * empty — the create flow uses it for "this match would be that one".
 */
export function PlacementsBar({
  played,
  required,
  pending = false,
  size = 'sm',
  className,
}: {
  played: number
  required: number
  /** Tint the segment after `played` as the one about to be filled. */
  pending?: boolean
  size?: 'sm' | 'lg'
  className?: string
}) {
  const label = pending
    ? `${played} of ${required} ranked matches played; this one would be the next`
    : `${played} of ${required} ranked matches played`

  return (
    <span
      role="img"
      aria-label={label}
      className={cn('flex', size === 'lg' ? 'gap-1' : 'gap-[3px]', className)}
    >
      {Array.from({ length: required }, (_, i) => (
        <span
          key={i}
          className={cn(
            'flex-1 rounded-full',
            size === 'lg' ? 'h-2' : 'h-1.5',
            i < played ? 'bg-primary' : pending && i === played ? 'bg-primary/40' : 'bg-muted',
          )}
        />
      ))}
    </span>
  )
}
