import { useState } from 'react'
import { CricketFields, FootballFields, NetballFields } from '../MatchFormatEditor'
import { ToggleRow } from './parts'
import type { MatchType } from '@/lib/sports'
import {
  DEFAULT_CRICKET_FORMAT,
  DEFAULT_FOOTBALL_FORMAT,
  DEFAULT_NETBALL_FORMAT,
  type CricketFormat,
  type FootballFormat,
  type MatchFormat,
  type NetballFormat,
} from '@/lib/matchFormat'
import { cn } from '@/lib/utils'

interface Preset {
  label: string
  apply: (sport: MatchType) => MatchFormat
  matches: (f: MatchFormat) => boolean
}

const footballPreset = (half: number): Preset => ({
  label: `2 × ${half} min`,
  apply: () => ({ sport: 'Football', ...DEFAULT_FOOTBALL_FORMAT, half_length_minutes: half }),
  matches: (f) => f.sport === 'Football' && f.num_halves === 2 && f.half_length_minutes === half,
})

const cricketPreset = (label: string, overs: number | undefined, innings: number): Preset => ({
  label,
  apply: () => ({
    sport: 'Cricket',
    ...DEFAULT_CRICKET_FORMAT,
    overs_per_innings: overs,
    innings_per_side: innings,
  }),
  matches: (f) =>
    f.sport === 'Cricket' &&
    (f.overs_per_innings ?? undefined) === overs &&
    f.innings_per_side === innings,
})

const netballPreset = (quarter: number): Preset => ({
  label: `4 × ${quarter} min`,
  apply: () => ({ sport: 'Netball', ...DEFAULT_NETBALL_FORMAT, quarter_length_minutes: quarter }),
  matches: (f) => f.sport === 'Netball' && f.num_quarters === 4 && f.quarter_length_minutes === quarter,
})

const PRESETS: Partial<Record<MatchType, Preset[]>> = {
  football: [footballPreset(20), footballPreset(30), footballPreset(45)],
  cricket: [cricketPreset('T20', 20, 1), cricketPreset('ODI', 50, 1), cricketPreset('Test', undefined, 2)],
  netball: [netballPreset(10), netballPreset(15)],
}

function defaultFormat(sport: MatchType): MatchFormat {
  if (sport === 'cricket') return { sport: 'Cricket', ...DEFAULT_CRICKET_FORMAT }
  if (sport === 'netball') return { sport: 'Netball', ...DEFAULT_NETBALL_FORMAT }
  return { sport: 'Football', ...DEFAULT_FOOTBALL_FORMAT }
}

const chip = (selected: boolean) =>
  cn(
    'h-10 rounded-full border px-3.5 text-sm transition-colors',
    selected
      ? 'border-foreground bg-foreground font-bold text-background'
      : 'border-input bg-card font-semibold hover:bg-accent/50',
  )

/**
 * Optional match format as one-tap presets (football halves, cricket T20 /
 * ODI / Test, netball quarters) plus "Custom" for the full field editor.
 * `null` means the app defaults apply. Renders nothing for sports with no
 * format modelled.
 */
export function FormatPresets({
  sport,
  value,
  onChange,
}: {
  sport: MatchType
  value: MatchFormat | null
  onChange: (value: MatchFormat | null) => void
}) {
  const presets = PRESETS[sport]
  const [custom, setCustom] = useState(false)
  if (!presets) return null

  const active = value && !custom ? presets.find((p) => p.matches(value)) : undefined
  const isCustom = custom || (value !== null && !active)
  const current = value ?? defaultFormat(sport)

  return (
    <div className="flex flex-col gap-3.5">
      <div className="flex flex-col gap-2">
        <span className="text-[13px] font-semibold text-muted-foreground">
          {sport === 'cricket' ? 'Format' : 'Length'}
        </span>
        <div role="radiogroup" aria-label="Match length" className="flex flex-wrap gap-2">
          {presets.map((p) => (
            <button
              key={p.label}
              type="button"
              role="radio"
              aria-checked={active === p}
              onClick={() => {
                setCustom(false)
                onChange(active === p ? null : p.apply(sport))
              }}
              className={chip(active === p)}
            >
              {p.label}
            </button>
          ))}
          <button
            type="button"
            role="radio"
            aria-checked={isCustom}
            onClick={() => {
              if (isCustom) {
                setCustom(false)
                onChange(null)
              } else {
                setCustom(true)
                onChange(current)
              }
            }}
            className={chip(isCustom)}
          >
            Custom
          </button>
        </div>
      </div>

      {isCustom && (
        <div className="border-t pt-3">
          {current.sport === 'Cricket' ? (
            <CricketFields value={current} onChange={(v: CricketFormat) => onChange({ sport: 'Cricket', ...v })} />
          ) : current.sport === 'Netball' ? (
            <NetballFields value={current} onChange={(v: NetballFormat) => onChange({ sport: 'Netball', ...v })} />
          ) : (
            <FootballFields value={current} onChange={(v: FootballFormat) => onChange({ sport: 'Football', ...v })} />
          )}
        </div>
      )}

      {!isCustom && current.sport === 'Football' && (
        <div className="flex flex-col border-t pt-1">
          <ToggleRow
            label="Extra time if level"
            hint={`2 × ${current.extra_time_half_length_minutes ?? 15} min`}
            checked={current.extra_time}
            onChange={(extra_time) =>
              onChange({
                ...current,
                extra_time,
                extra_time_half_length_minutes: extra_time
                  ? (current.extra_time_half_length_minutes ?? 15)
                  : undefined,
              })
            }
          />
          <ToggleRow
            label="Penalties if still level"
            checked={current.penalties}
            onChange={(penalties) => onChange({ ...current, penalties })}
          />
        </div>
      )}

      {!isCustom && current.sport === 'Netball' && (
        <div className="flex flex-col border-t pt-1">
          <ToggleRow
            label="Extra time if level"
            checked={current.extra_time}
            onChange={(extra_time) => onChange({ ...current, extra_time })}
          />
        </div>
      )}
    </div>
  )
}
