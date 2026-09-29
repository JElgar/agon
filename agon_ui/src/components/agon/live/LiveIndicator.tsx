import { cn } from '@/lib/utils'

/** Pulsing "LIVE" pill — used in place of `StatusBadge` wherever a football
 *  match is in progress and being scored live. An optional prefix (e.g. a
 *  minute label) renders before "LIVE", as in "63' LIVE".
 *
 *  `variant`: `subtle` (default) is the tinted chip used on read-only match
 *  cards/details; `solid` is the filled destructive pill from the live-
 *  scoring screens' header (see the "Agon redesign" canvas,
 *  claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc). */
export function LiveIndicator({
  className,
  children,
  variant = 'subtle',
}: {
  className?: string
  children?: React.ReactNode
  variant?: 'subtle' | 'solid'
}) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center gap-1.5 rounded-full text-[10px] font-bold tracking-wide',
        variant === 'solid'
          ? 'bg-destructive px-2.5 py-1 text-destructive-foreground'
          : 'gap-1 rounded border border-destructive/30 bg-destructive/10 px-1.5 py-0.5 font-medium text-destructive',
        className,
      )}
    >
      <span className="relative flex size-1.5">
        <span
          className={cn(
            'absolute inline-flex size-full animate-ping rounded-full opacity-75',
            variant === 'solid' ? 'bg-destructive-foreground' : 'bg-destructive',
          )}
        />
        <span
          className={cn(
            'relative inline-flex size-1.5 rounded-full',
            variant === 'solid' ? 'bg-destructive-foreground' : 'bg-destructive',
          )}
        />
      </span>
      {children ? <>{children} LIVE</> : 'LIVE'}
    </span>
  )
}
