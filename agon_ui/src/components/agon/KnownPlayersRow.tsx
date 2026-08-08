import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Avatar } from './Avatar'

type UserProfile = components['schemas']['UserProfile']

export interface KnownPlayersRowProps extends React.HTMLAttributes<HTMLDivElement> {
  /** Up to `MAX_KNOWN_PLAYERS` participants the viewer follows — a feed
   *  card's `FeedMatch.known_participants`. */
  participants: UserProfile[]
  /** The true, uncapped total the participants were followed out of — a feed
   *  card's `FeedMatch.known_participants_count`. May exceed
   *  `participants.length`, in which case the row falls back to "+N" for the
   *  rest. */
  count: number
}

/** Avatars rendered in the overlapping cluster, regardless of how many
 *  hydrated profiles are available. */
const MAX_AVATARS = 3
/** Participants named directly before the rest collapse into "+N". */
const MAX_NAMED = 2

/**
 * "You follow Sofia, Raj +1" — shown on a feed card when the viewer follows
 * one or more of the match's participants. `count` (not
 * `participants.length`) drives the "+N" suffix, since `participants` is
 * capped server-side (`agon_core::dao::audience::MAX_KNOWN_PLAYERS`) while
 * `count` is the true total — so the row stays accurate even when more
 * participants are followed than were hydrated. Renders nothing if `count`
 * is 0 (the viewer doesn't follow anyone playing, or this is their own
 * match).
 */
export function KnownPlayersRow({
  participants,
  count,
  className,
  ...props
}: KnownPlayersRowProps) {
  if (count === 0) return null

  const avatars = participants.slice(0, MAX_AVATARS)
  const named = participants.slice(0, MAX_NAMED)
  const remaining = count - named.length

  return (
    <div className={cn('flex items-center gap-2.5', className)} {...props}>
      <div className="flex -space-x-2">
        {avatars.map((p) => (
          <Avatar
            key={p.id}
            name={p.name}
            imageUrl={p.profile_image?.image_url}
            size="sm"
            className="ring-2 ring-card"
          />
        ))}
      </div>
      <p className="min-w-0 truncate text-xs text-muted-foreground">
        You follow{' '}
        <span className="font-medium text-foreground">
          {named.map((p) => p.name).join(', ')}
        </span>
        {remaining > 0 && ` +${remaining}`}
      </p>
    </div>
  )
}
