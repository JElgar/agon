import { cn } from '@/lib/utils'

/** Pulsing "LIVE" pill — used in place of `StatusBadge` wherever a football
 *  match is in progress and being scored live. An optional prefix (e.g. a
 *  minute label) renders before "LIVE", as in "63' LIVE". */
export function LiveIndicator({
  className,
  children,
}: {
  className?: string
  children?: React.ReactNode
}) {
  return (
    <span
      className={cn(
        'inline-flex items-center gap-1 rounded border border-destructive/30 bg-destructive/10 px-1.5 py-0.5 text-[10px] font-medium text-destructive',
        className,
      )}
    >
      <span className="relative flex size-1.5">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-destructive opacity-75" />
        <span className="relative inline-flex size-1.5 rounded-full bg-destructive" />
      </span>
      {children ? <>{children} LIVE</> : 'LIVE'}
    </span>
  )
}
