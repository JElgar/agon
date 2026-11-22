import type { ReactNode } from 'react'
import { cn } from '@/lib/utils'

/**
 * Sticks to the bottom of the page: flush with the viewport bottom on
 * mobile (where the bottom tab bar is hidden for pages that render one of
 * these — see `hidesMobileBottomNav` in `App.tsx`) and a rounded card at
 * the `md` breakpoint and up. Relies on its parent being the page's root
 * flex column with `mt-auto` doing the pinning, sized via `min-h-[calc(…)]`
 * and a matching negative bottom margin that cancels `<main>`'s own bottom
 * padding — see `MatchDetailPage`'s and `LogMatchPage`'s root wrapper.
 *
 * `className` carries each use's own flex direction/gap and desktop
 * treatment (see `MatchActionBar`, `ScheduledMatchInvite`, `LogMatchPage`).
 */
export function PinnedActionBar({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div
      className={cn(
        'sticky bottom-0 z-10 -mx-4 mt-auto bg-card/95 px-4 pt-3 pb-[max(12px,env(safe-area-inset-bottom))] backdrop-blur supports-[backdrop-filter]:bg-card/80',
        className,
      )}
    >
      {children}
    </div>
  )
}
