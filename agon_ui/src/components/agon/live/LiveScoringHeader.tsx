import { ChevronLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { LiveIndicator } from './LiveIndicator'

/**
 * The chrome shared by every live-scoring screen (football, cricket,
 * netball, and the pre-match setup page): a round back button, the screen
 * title, and — once scoring's actually live — the solid "LIVE" pill. See the
 * "Agon redesign" canvas's live-scoring mock
 * (claude.ai/artifact/MKvQ8bNeKnqHzxqZfMNFnc): "Live scoring" header with a
 * red "LIVE" pill.
 *
 * `right` is for a screen-specific slot next to the pill (currently just
 * `UndoLastEventButton`) — kept as a slot rather than baked in here since the
 * setup page has nothing to undo yet.
 */
export function LiveScoringHeader({
  title = 'Live scoring',
  onBack,
  right,
  live = true,
}: {
  title?: string
  onBack: () => void
  right?: React.ReactNode
  live?: boolean
}) {
  return (
    <div className="flex items-center gap-1">
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="shrink-0 rounded-full text-foreground"
        onClick={onBack}
        aria-label="Back"
      >
        <ChevronLeft className="size-5" />
      </Button>
      <h1 className="flex-1 truncate font-display text-lg font-extrabold">{title}</h1>
      {right}
      {live && <LiveIndicator variant="solid" />}
    </div>
  )
}
