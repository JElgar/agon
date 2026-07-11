import { cn } from '@/lib/utils'
import { sportIcon, sportLabel, type MatchType } from '@/lib/sports'

/** The sports offered, in display order. Mirrors the `MatchType` enum. */
const SPORTS: MatchType[] = [
  'tennis',
  'badminton',
  'squash',
  'table_tennis',
  'football',
  'cricket',
  'other',
]

export interface SportPickerProps {
  /** The currently selected sport, or `null` when nothing is picked yet. */
  value: MatchType | null
  onChange: (sport: MatchType) => void
}

/**
 * The sport-selection grid for the "Log a match" flow: one tappable tile per
 * `MatchType`, the selected one highlighted with the primary accent. Reuses the
 * shared sport icon/label mapping so tiles match the rest of the app.
 */
export function SportPicker({ value, onChange }: SportPickerProps) {
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
      {SPORTS.map((sport) => {
        const Icon = sportIcon(sport)
        const selected = value === sport
        return (
          <button
            key={sport}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(sport)}
            className={cn(
              'flex flex-col items-center gap-1.5 rounded-lg border bg-muted/40 p-3 transition-colors',
              selected
                ? 'border-primary bg-accent text-accent-foreground'
                : 'text-muted-foreground hover:bg-muted',
            )}
          >
            <Icon className={cn('size-6', selected && 'text-primary')} />
            <span className="text-xs font-medium">{sportLabel(sport)}</span>
          </button>
        )
      })}
    </div>
  )
}
