import { Star, TriangleAlert } from 'lucide-react'
import { sportLabel, type MatchType } from '@/lib/sports'
import {
  confidenceLabel,
  placementsLeft,
  type LadderPlacement,
  type LadderRating,
} from '@/lib/rating'
import { cn } from '@/lib/utils'
import { PlacementsBar } from '../RatingDisplay'
import { CARD, FormSection, Segmented } from './parts'

export interface HowItCountsProps {
  sport: MatchType
  ranked: boolean
  onChange: (ranked: boolean) => void
  /** True while the create flow is building a recurring series, which can't
   *  be ranked — `POST /match-series` has no `ranked` field at all. */
  seriesMode: boolean
  /** The organiser's own published rating on this sport's ladder, if any. */
  rating?: LadderRating
  /** The organiser's own placement progress, if the rating isn't published. */
  placement?: LadderPlacement
  /** Names of players tagged onto a side who have no Agon account. The server
   *  refuses to create a ranked match containing any of them, so this blocks
   *  the step rather than warning about it — see `GuestRefusal`. */
  guests?: string[]
}

/**
 * The create flow's ranked-or-friendly choice — the EloCreate board on the
 * "Agon redesign" canvas (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 *
 * This single control is the whole of what a player agrees to. There is no
 * opt-in, no tickbox and no per-player consent: the organiser picks ranked
 * here, and playing a ranked match *is* the agreement. The board used to
 * carry a consent checkbox and an "if they say no, the match stays friendly"
 * rule, and both are gone from the model — a prompt here would be asking
 * about something that no longer exists.
 *
 * Friendly is the default, matching `CreateMatchInput.ranked`'s own default:
 * most games are casual, so ranked is a deliberate choice rather than
 * something a kickabout falls into.
 *
 * What it shows below the choice is the organiser's *own* standing on that
 * ladder, never anybody else's — not because there's nothing to show, but
 * because `rating_placements` is private to its owner, and a card saying how
 * far through placements the opponent is would be handing over exactly the
 * hint the API withholds.
 */
export function HowItCounts({
  sport,
  ranked,
  onChange,
  seriesMode,
  rating,
  placement,
  guests = [],
}: HowItCountsProps) {
  const ladder = sportLabel(sport).toLowerCase()

  if (seriesMode) {
    return (
      <FormSection title="How it counts">
        <div className={cn(CARD, 'gap-1.5')}>
          <span className="text-sm font-semibold">Repeating matches are friendly</span>
          <span className="text-[13px] text-muted-foreground">
            A series can't be created ranked yet, so its matches are friendlies.
          </span>
        </div>
      </FormSection>
    )
  }

  return (
    <FormSection title="How it counts">
      <Segmented
        label="Friendly or ranked"
        value={ranked ? 'ranked' : 'friendly'}
        onChange={(v) => onChange(v === 'ranked')}
        options={[
          { value: 'friendly', label: 'Friendly' },
          {
            value: 'ranked',
            label: (
              <span className="flex items-center justify-center gap-1.5">
                <Star className="size-3.5" />
                Ranked
              </span>
            ),
          },
        ]}
      />
      <span className="px-1 text-[13px] leading-snug text-muted-foreground">
        {ranked
          ? `Moves every player's ${ladder} rating once the score is agreed. Playing is the agreement — there is no separate opt-in.`
          : 'Counts in your stats and head-to-heads. No effect on ratings.'}
      </span>
      {ranked && guests.length > 0 && <GuestRefusal guests={guests} />}
      {ranked && <YourStanding ladder={ladder} rating={rating} placement={placement} />}
    </FormSection>
  )
}

/**
 * A ranked match can't contain a guest, and `POST /matches` refuses one
 * outright — so this is a block, not a warning.
 *
 * The reason is that such a match would rate *nobody*: there's no account to
 * key a rating to, and Weng-Lin sums a side's beliefs, so rating around a
 * guest would credit their share of the result to their team-mates. The
 * worker therefore skips the whole match rather than rate part of it, which
 * before the server-side refusal meant the organiser picked ranked, everyone
 * played, the score was agreed, and nothing moved — with nothing said
 * anywhere but a `warn` in the worker.
 *
 * This mirrors `mapping::ranked_guest_invites_reason`, down to offering the
 * same two ways out in the same order. It is a mirror and not the only
 * guard: the server refuses regardless, and the point here is to say so
 * while the choice is still free, rather than let the organiser reach the
 * end of the flow and be turned away by a 400.
 *
 * Naming them is the whole value. "Remove your guests" sends you hunting
 * through two sides; naming them is the fix.
 */
function GuestRefusal({ guests }: { guests: string[] }) {
  const names =
    guests.length === 1
      ? guests[0]
      : `${guests.slice(0, -1).join(', ')} and ${guests[guests.length - 1]}`

  return (
    <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2.5 text-[13px] leading-snug text-destructive">
      <TriangleAlert className="mt-0.5 size-4 shrink-0" />
      <span>
        <b className="font-semibold">
          A ranked match can't include {names}.
        </b>{' '}
        They have no Agon account, and a ranked match rates everyone or no one. Invite them to
        Agon, or make this match friendly.
      </span>
    </div>
  )
}

/** Where picking "ranked" leaves the organiser: their rating about to move,
 *  their placements about to advance, or their first one about to start. */
function YourStanding({
  ladder,
  rating,
  placement,
}: {
  ladder: string
  rating?: LadderRating
  placement?: LadderPlacement
}) {
  if (rating) {
    return (
      <div className={cn(CARD, 'gap-2')}>
        <div className="flex items-baseline justify-between gap-3">
          <span className="text-sm font-bold">Your {ladder} rating</span>
          <span className="flex items-baseline gap-1.5">
            <span className="font-display text-[20px] leading-none font-extrabold tabular-nums">
              {rating.rating}
            </span>
            <span className="text-[13px] font-semibold text-muted-foreground">
              {confidenceLabel(rating.confidence)}
            </span>
          </span>
        </div>
        <p className="text-xs leading-relaxed text-muted-foreground">
          This result will move it once the score is agreed.
        </p>
      </div>
    )
  }

  if (!placement) {
    return (
      <div className={cn(CARD, 'gap-2')}>
        <span className="text-sm font-bold">Your first ranked {ladder} match</span>
        <p className="text-xs leading-relaxed text-muted-foreground">
          This would start your placements. Your rating stays hidden until they're done — and
          only you can see your own progress, never anyone else's.
        </p>
      </div>
    )
  }

  const last = placementsLeft(placement) === 1

  return (
    <div className={cn(CARD, 'gap-3')}>
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-sm font-bold">Your {ladder} placements</span>
        <span className="text-[13px] font-semibold text-muted-foreground">
          {placement.played} of {placement.required}
        </span>
      </div>
      <PlacementsBar played={placement.played} required={placement.required} pending />
      <p className="text-xs leading-relaxed text-muted-foreground">
        {last
          ? `This would be the last of your ${placement.required}, so your ${ladder} rating starts showing on your profile once the score is agreed. `
          : `This would be ${placement.played + 1} of your ${placement.required}. Your rating starts showing once they're done. `}
        Only you can see your own progress — never anyone else's.
      </p>
    </div>
  )
}
