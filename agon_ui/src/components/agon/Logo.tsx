import { Link } from 'react-router-dom'
import { cn } from '@/lib/utils'

/**
 * The Agon wordmark: a small blue ring beside the italic serif brand name.
 * Shared by the desktop sidebar header, the tablet header and the mobile
 * top bar so the chrome surfaces read as one brand. Links to the feed so
 * it also acts as a "home" shortcut everywhere it appears.
 */
export function Logo({ className }: { className?: string }) {
  return (
    <Link
      to="/feed"
      aria-label="Agon home"
      className={cn('inline-flex items-center gap-2', className)}
    >
      <span className="size-4 shrink-0 rounded-full border-[3px] border-primary" />
      <span className="font-serif text-2xl font-semibold italic leading-none">
        Agon
      </span>
    </Link>
  )
}
