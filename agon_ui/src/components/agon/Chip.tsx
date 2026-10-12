import { cn } from '@/lib/utils'

export interface ChipProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  pressed: boolean
}

/**
 * A filter-toggle pill (e.g. "All" / "Won" / "Lost" chips above a match
 * list) — solid dark when pressed, outlined when not. See the "Agon
 * redesign" canvas's profile/sport-stats filter rows.
 */
export function Chip({ pressed, className, ...props }: ChipProps) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      className={cn(
        'h-9 shrink-0 whitespace-nowrap rounded-full border px-3.5 text-sm font-semibold transition-colors',
        pressed
          ? 'border-foreground bg-foreground text-background'
          : 'border-input bg-background text-foreground hover:bg-accent hover:text-accent-foreground',
        className,
      )}
      {...props}
    />
  )
}
