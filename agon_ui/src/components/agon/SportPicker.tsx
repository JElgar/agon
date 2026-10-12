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
  'netball',
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
    <div className="flex flex-wrap gap-2">
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
              'inline-flex h-9 items-center gap-2 rounded-full border px-3.5 text-sm font-semibold transition-colors',
              selected
                ? 'border-primary bg-accent text-accent-foreground'
                : 'border-input bg-card text-muted-foreground hover:bg-accent/50',
            )}
          >
            <Icon className={cn('size-4', selected && 'text-primary')} />
            {sportLabel(sport)}
          </button>
        )
      })}
    </div>
  )
}
