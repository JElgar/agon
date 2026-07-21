import { cn } from '@/lib/utils'

/**
 * The Agon wordmark: a small blue ring beside the italic serif brand name.
 * Shared by the desktop sidebar header and the mobile top bar so the two
 * chrome surfaces read as one brand.
 */
export function Logo({ className }: { className?: string }) {
  return (
    <span className={cn('inline-flex items-center gap-2', className)}>
      <span className="size-4 shrink-0 rounded-full border-[3px] border-primary" />
      <span className="font-serif text-2xl font-semibold italic leading-none">
        Agon
      </span>
    </span>
  )
}
