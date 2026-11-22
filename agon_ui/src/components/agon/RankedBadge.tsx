import { Star } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * The pill marking a match whose result counts towards everyone's rating —
 * the "Ranked" chip on the EloMatchRatings board of the "Agon redesign"
 * canvas (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc).
 *
 * Only ever shown on a ranked match. There's no friendly counterpart on
 * purpose: nearly every match is a friendly, so a chip on each of them would
 * be a badge for "normal" — the mark worth making is the one that says this
 * result will move numbers.
 */
export function RankedBadge({ className, ...props }: React.HTMLAttributes<HTMLSpanElement>) {
  return (
    <span
      className={cn(
        'inline-flex h-6 shrink-0 items-center gap-1.5 rounded-full bg-accent px-2.5 text-[11px] font-extrabold text-accent-foreground',
        className,
      )}
      {...props}
    >
      <Star className="size-3" strokeWidth={2.4} />
      Ranked
    </span>
  )
}
