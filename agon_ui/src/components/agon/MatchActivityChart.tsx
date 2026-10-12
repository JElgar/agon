import { cn } from '@/lib/utils'

export interface MonthlyActivityPoint {
  /** Short month label, e.g. "Mar". */
  label: string
  /** Full label for the tooltip, e.g. "March 2026". */
  fullLabel: string
  count: number
}

/**
 * Matches-per-month bar chart: one column per month, its count printed above
 * the bar, a muted flat bar for a zero month, the most recent month picked
 * out in the full brand blue (see the "Sport" board of the "Agon redesign"
 * canvas, claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 */
export function MatchActivityChart({ data, className }: { data: MonthlyActivityPoint[]; className?: string }) {
  const max = Math.max(1, ...data.map((d) => d.count))
  const lastIndex = data.length - 1

  return (
    <div
      role="img"
      aria-label={`Matches per month: ${data.map((d) => `${d.fullLabel}, ${d.count} match${d.count === 1 ? '' : 'es'}`).join('; ')}`}
      className={cn('grid items-end gap-2', className)}
      style={{ gridTemplateColumns: `repeat(${data.length}, minmax(0, 1fr))`, height: 170 }}
    >
      {data.map((d, i) => {
        const barHeight = d.count === 0 ? 8 : Math.max(24, (d.count / max) * 120)
        const isCurrent = i === lastIndex
        return (
          <div key={d.fullLabel} className="flex h-full flex-col items-center justify-end gap-1.5" aria-hidden>
            <span className={cn('text-xs', d.count === 0 ? 'text-muted-foreground' : 'font-bold text-foreground')}>
              {d.count === 0 ? '–' : d.count}
            </span>
            <span
              className={cn('w-8 rounded-lg', d.count === 0 ? 'bg-muted' : isCurrent ? 'bg-primary' : 'bg-primary/40')}
              style={{ height: barHeight }}
            />
            <span className={cn('text-xs', isCurrent ? 'font-bold text-foreground' : 'font-medium text-muted-foreground')}>
              {d.label}
            </span>
          </div>
        )
      })}
    </div>
  )
}
