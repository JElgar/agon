import { useEffect, useState } from 'react'
import { Check, Flame, MailOpen, MessageCircle, Share2 } from 'lucide-react'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { useToggleLike } from '@/hooks/useToggleLike'
import { relativeTime } from '@/lib/datetime'
import { Avatar } from './Avatar'
import { SportBadge } from './SportBadge'
import { StatusBadge, matchBadgeStatus } from './StatusBadge'
import { ScoreConfirmationBar } from './ScoreConfirmationBar'
import { MatchHeaderCarousel } from './MatchHeaderCarousel'
import {
  displayScore,
  headlineBySide,
  headlineLabel,
  setLine,
} from '@/lib/score'
import { myPendingInvitation } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']

export interface MatchCardProps extends React.HTMLAttributes<HTMLDivElement> {
  match: Match
  /** Called when the card body is activated (navigate to match detail). */
  onOpen?: () => void
  /** The signed-in user's id. When they're a participant with a pending score to
   *  respond to, an inline confirm/dispute prompt is shown. */
  currentUserId?: string
}

/** Display label for a side: its name, or a neutral fallback. */
function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/** A small pill shown when the viewer has a pending invitation to this match. */
function InvitedBadge({
  match,
  currentUserId,
}: {
  match: Match
  currentUserId?: string
}) {
  if (!myPendingInvitation(match, currentUserId)) return null
  return (
    <span className="inline-flex items-center gap-1 rounded border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary">
      <MailOpen className="size-3" /> You're invited
    </span>
  )
}

/**
 * Shares a match's detail link via the native share sheet, falling back to a
 * clipboard copy (then a manual prompt) where that's unavailable — mirrors
 * `CopyInviteButton`'s fallback chain, with a transient checkmark standing in
 * for its "Copied!" label since this is an icon-only button.
 */
function ShareMatchButton({ match }: { match: Match }) {
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const id = setTimeout(() => setCopied(false), 2000)
    return () => clearTimeout(id)
  }, [copied])

  const share = async () => {
    const url = `${window.location.origin}/matches/${match.id}`
    if (navigator.share) {
      try {
        await navigator.share({ title: match.name, url })
        return
      } catch {
        // User dismissed the sheet, or share failed — fall through to copy.
      }
    }
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
    } catch {
      // Clipboard blocked (e.g. insecure context) — surface the link so the
      // user can copy it manually rather than failing silently.
      window.prompt('Copy this match link:', url)
    }
  }

  return (
    <button
      type="button"
      onClick={share}
      className="flex items-center transition-colors hover:text-primary"
      aria-label="Share match"
    >
      {copied ? (
        <Check className="size-3.5 text-primary" />
      ) : (
        <Share2 className="size-3.5" />
      )}
    </button>
  )
}

/**
 * A match card for the feed: the two sides, the score, sport, confirmation
 * state, and social actions. Presentational — data comes from a `Match`
 * (which the feed's `FeedItem_Match` extends); callers wire the action handlers.
 */
export function MatchCard({
  match,
  onOpen,
  currentUserId,
  className,
  ...props
}: MatchCardProps) {
  const [sideA, sideB] = match.sides
  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}
  const sets = scoreInfo ? setLine(scoreInfo.score, match.sides) : []

  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')
  const aWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideA?.id
  const bWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideB?.id

  const { like_count, comment_count, i_liked } = match.social
  const toggleLike = useToggleLike(match)

  return (
    <div
      className={cn(
        'overflow-hidden rounded-xl border bg-card text-card-foreground',
        className,
      )}
      {...props}
    >
      {/* Header: who beat who + when + sport */}
      <button
        type="button"
        onClick={onOpen}
        className="flex w-full items-start justify-between gap-3 p-3.5 text-left"
      >
        <p className="text-sm leading-snug">
          <span className={cn(aWon && 'font-medium')}>{nameA}</span>
          <span className="text-primary">
            {' '}
            {scoreInfo?.winnerSideId ? 'beat' : 'vs'}{' '}
          </span>
          <span className={cn(bWon && 'font-medium')}>{nameB}</span>
          <span className="text-muted-foreground"> · {relativeTime(match.starts_at)}</span>
        </p>
        <div className="flex shrink-0 flex-col items-end gap-1.5">
          <SportBadge sport={match.match_type} />
          <InvitedBadge match={match} currentUserId={currentUserId} />
        </div>
      </button>

      {/* Title + description */}
      {(match.name || match.description) && (
        <div className="px-3.5 pb-3">
          {match.name && <p className="font-medium leading-snug">{match.name}</p>}
          {match.description && (
            <p className="mt-0.5 text-sm text-muted-foreground">
              {match.description}
            </p>
          )}
        </div>
      )}

      {/* Score block */}
      {scoreInfo && (
        <div className="mx-3.5 rounded-lg bg-muted/50 px-3.5 py-3">
          <div className="flex items-center justify-between">
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <Avatar name={nameA} size="md" ring={aWon ? 'winner' : 'none'} />
              <span className="truncate text-xs font-medium">{nameA}</span>
            </div>
            <div className="px-3 text-center">
              <div className="text-2xl font-medium leading-none tracking-tight">
                {headline[sideA?.id ?? ''] ?? 0}
                <span className="text-muted-foreground">–</span>
                {headline[sideB?.id ?? ''] ?? 0}
              </div>
              <div className="mt-0.5 text-[9px] uppercase tracking-widest text-muted-foreground">
                {headlineLabel(scoreInfo.score)}
              </div>
            </div>
            <div className="flex min-w-0 flex-1 flex-row-reverse items-center gap-2 text-right">
              <Avatar name={nameB} size="md" ring={bWon ? 'winner' : 'none'} />
              <span className="truncate text-xs font-medium">{nameB}</span>
            </div>
          </div>
          {sets.length > 0 && (
            <div className="mt-2 border-t pt-2 text-center text-[11px] text-muted-foreground">
              {sets.map((s, i) => (
                <span key={i}>
                  {i > 0 && <span className="mx-1.5 text-border">·</span>}
                  Set {i + 1} <span className="font-medium text-foreground">{s}</span>
                </span>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Header photo, when the match has one. */}
      {match.header_photos.length > 0 && (
        <div className="px-3.5 pb-3 pt-3">
          <MatchHeaderCarousel photos={match.header_photos} />
        </div>
      )}

      {/* Confirm/dispute prompt when the viewer's side owes a response. */}
      {match.pending_score && (
        <div className="px-3.5 pb-2.5">
          <ScoreConfirmationBar
            match={match}
            currentUserId={currentUserId}
            variant="card"
          />
        </div>
      )}

      {/* Footer: kudos + comments on the left, lifecycle/confirmation state on the right. */}
      <div className="flex items-center gap-4 border-t px-3.5 py-2.5 text-muted-foreground">
        <button
          type="button"
          onClick={() => toggleLike.mutate(!i_liked)}
          aria-pressed={i_liked}
          aria-label={i_liked ? 'Remove kudos' : 'Give kudos'}
          className={cn(
            'flex items-center gap-1.5 text-xs transition-colors hover:text-primary',
            i_liked && 'text-primary',
          )}
        >
          <Flame className={cn('size-3.5', i_liked && 'fill-current')} />{' '}
          {like_count} kudos
        </button>
        <button
          type="button"
          onClick={onOpen}
          className="flex items-center gap-1.5 text-xs transition-colors hover:text-primary"
        >
          <MessageCircle className="size-3.5" /> {comment_count}
        </button>
        <ShareMatchButton match={match} />
        <StatusBadge status={matchBadgeStatus(match)} className="ml-auto" />
      </div>
    </div>
  )
}
