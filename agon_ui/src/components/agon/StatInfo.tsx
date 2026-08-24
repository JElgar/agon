import { Info } from 'lucide-react'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

export interface StatInfoProps {
  /** Short explanation of how the stat next to this icon is computed. */
  children: React.ReactNode
}

/**
 * A small "?" affordance for a stat whose label alone doesn't say how it's
 * computed (strike rate, average, economy, ...) — tap/click opens a popover
 * with the explanation. Click-based rather than hover-only so it works the
 * same on mobile (no hover) and desktop, one tap either way.
 */
export function StatInfo({ children }: StatInfoProps) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label="What is this stat?"
          onClick={(e) => e.stopPropagation()}
          className="inline-flex size-3.5 items-center justify-center rounded-full text-muted-foreground/70 transition-colors hover:text-foreground"
        >
          <Info className="size-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="center"
        className="w-56 p-2.5 text-xs text-muted-foreground"
      >
        {children}
      </PopoverContent>
    </Popover>
  )
}
