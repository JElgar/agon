import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { relativeTime } from '@/lib/datetime'
import { sportLabel } from '@/lib/sports'
import { displayScore, headlineBySide } from '@/lib/score'

type SearchMatch = components['schemas']['SearchMatch']
type MatchOutcome = components['schemas']['MatchOutcome']

/** Badge letter + colors for a match's result — from `outcome` when the
 *  match is confirmed, "LIVE" while in progress, else a plain "?" (a
 *  scheduled match with no result yet, which the profile lists exclude via
 *  their `outcome`/`status` filtering, but keep this total just in case). */
function badgeFor(match: SearchMatch): { label: string; bg: string; fg: string; small?: boolean } {
  if (match.status === 'in_progress') {
    return { label: 'LIVE', bg: 'bg-destructive', fg: 'text-destructive-foreground', small: true }
  }
  const outcome: MatchOutcome | undefined = match.outcome
  if (outcome === 'won') return { label: 'W', bg: 'bg-accent', fg: 'text-accent-foreground' }
  if (outcome === 'lost') return { label: 'L', bg: 'bg-destructive/15', fg: 'text-destructive' }
  if (outcome === 'draw') return { label: 'D', bg: 'bg-muted', fg: 'text-muted-foreground' }
  return { label: '–', bg: 'bg-muted', fg: 'text-muted-foreground' }
}

/** The score/headline text on the right of a row, e.g. "35–16" — the same
 *  headline math `MatchCard` uses, condensed to a dash-joined string (no
 *  per-set breakdown; a scheduled match with nothing to show yet). */
function scoreLabel(match: SearchMatch): string {
  const info = displayScore(match)
  if (!info) return match.status === 'in_progress' ? '•' : ''
  const headline = headlineBySide(info.score)
  const values = match.sides.map((s) => headline[s.id] ?? 0)
  if (values.length < 2 || match.match_type === 'cricket') {
    // Cricket's headline isn't a plain per-side number (see
    // `headlineBySide`'s doc comment) — fall back to the raw total off
    // whichever side has one, same as the design mock's "142/6".
    return ''
  }
  return values.join('–')
}

export interface ProfileMatchRowProps extends React.HTMLAttributes<HTMLButtonElement> {
  match: SearchMatch
  onOpen?: () => void
  /** Shows a small "Teammates" tag next to the title (the "playing
   *  together" case in the head-to-head view). */
  teammates?: boolean
  isFirst?: boolean
}

/**
 * Compact match row — badge, title/subtitle, score — used by the profile's
 * "Your matches" and "Matches together" lists (`Profile.dc.html` on the
 * "Agon redesign" canvas). Deliberately not `MatchCard`: the mock's list rows
 * are a single line, not the full social card the feed uses.
 */
export function ProfileMatchRow({
  match,
  onOpen,
  teammates,
  isFirst,
  className,
  ...props
}: ProfileMatchRowProps) {
  const badge = badgeFor(match)
  const sideNames = match.sides.map((s) => s.name).filter(Boolean).join(' vs ')

  return (
    <button
      type="button"
      onClick={onOpen}
      className={cn(
        'flex min-h-[60px] w-full items-center gap-3 px-4 py-3 text-left',
        !isFirst && 'border-t',
        className,
      )}
      {...props}
    >
      <span
        className={cn(
          'flex size-8 shrink-0 items-center justify-center rounded-[10px] font-display font-extrabold tracking-wide',
          badge.small ? 'text-[9px]' : 'text-[13px]',
          badge.bg,
          badge.fg,
        )}
      >
        {badge.label}
      </span>
      <span className="flex min-w-0 flex-grow flex-col gap-0.5">
        <span className="flex items-center gap-1.5">
          <span className="truncate text-[15px] font-semibold">{match.name}</span>
          {teammates && (
            <span className="inline-flex h-[18px] shrink-0 items-center rounded-full bg-muted px-1.5 text-[10px] font-bold text-muted-foreground">
              Teammates
            </span>
          )}
        </span>
        <span className="truncate text-[13px] text-muted-foreground">
          {sportLabel(match.match_type)} &middot; {sideNames || match.name} &middot;{' '}
          {relativeTime(match.starts_at)}
        </span>
      </span>
      <span className="font-display text-lg font-extrabold">{scoreLabel(match)}</span>
    </button>
  )
}
