import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { MatchType } from '@/lib/sports'
import {
  DEFAULT_CRICKET_FORMAT,
  DEFAULT_FOOTBALL_FORMAT,
  type CricketFormat,
  type FootballFormat,
  type MatchFormat,
} from '@/lib/matchFormat'

/** A labelled number field, small enough to sit a few to a row. */
function NumberField({
  label,
  value,
  onChange,
  min = 0,
  placeholder,
}: {
  label: string
  value: number | undefined
  onChange: (value: number | undefined) => void
  min?: number
  placeholder?: string
}) {
  return (
    <div className="flex flex-col gap-1">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      <Input
        type="number"
        min={min}
        inputMode="numeric"
        value={value ?? ''}
        placeholder={placeholder}
        onChange={(e) => {
          const n = Number(e.target.value)
          onChange(e.target.value === '' || !Number.isFinite(n) ? undefined : n)
        }}
      />
    </div>
  )
}

function ToggleRow({
  label,
  checked,
  onChange,
}: {
  label: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <div className="flex items-center justify-between py-1">
      <Label className="text-sm">{label}</Label>
      <Switch checked={checked} onCheckedChange={onChange} />
    </div>
  )
}

export function FootballFields({
  value,
  onChange,
}: {
  value: FootballFormat
  onChange: (value: FootballFormat) => void
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2">
        <NumberField
          label="Half length (min)"
          value={value.half_length_minutes}
          onChange={(v) => onChange({ ...value, half_length_minutes: v ?? 0 })}
        />
        <NumberField
          label="Halves"
          value={value.num_halves}
          onChange={(v) => onChange({ ...value, num_halves: v ?? 0 })}
        />
      </div>
      <ToggleRow
        label="Extra time if level"
        checked={value.extra_time}
        onChange={(extra_time) => onChange({ ...value, extra_time })}
      />
      {value.extra_time && (
        <NumberField
          label="Extra-time half length (min)"
          value={value.extra_time_half_length_minutes}
          onChange={(v) => onChange({ ...value, extra_time_half_length_minutes: v })}
        />
      )}
      <ToggleRow
        label="Penalty shootout if still level"
        checked={value.penalties}
        onChange={(penalties) => onChange({ ...value, penalties })}
      />
    </div>
  )
}

export function CricketFields({
  value,
  onChange,
}: {
  value: CricketFormat
  onChange: (value: CricketFormat) => void
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-2">
        <NumberField
          label="Overs per innings"
          value={value.overs_per_innings ?? undefined}
          placeholder="Unlimited"
          onChange={(v) => onChange({ ...value, overs_per_innings: v })}
        />
        <NumberField
          label="Innings per side"
          value={value.innings_per_side}
          min={1}
          onChange={(v) => onChange({ ...value, innings_per_side: v ?? 1 })}
        />
      </div>
      <div className="grid grid-cols-2 gap-2">
        <NumberField
          label="Balls per over"
          value={value.balls_per_over}
          min={1}
          onChange={(v) => onChange({ ...value, balls_per_over: v ?? 6 })}
        />
      </div>
      <div className="grid grid-cols-2 gap-2">
        <NumberField
          label="No-ball penalty (runs)"
          value={value.no_ball_penalty_runs}
          min={1}
          onChange={(v) => onChange({ ...value, no_ball_penalty_runs: v ?? 1 })}
        />
        <NumberField
          label="Wide penalty (runs)"
          value={value.wide_penalty_runs}
          min={1}
          onChange={(v) => onChange({ ...value, wide_penalty_runs: v ?? 1 })}
        />
      </div>
      <ToggleRow
        label="Free hit after a no-ball"
        checked={value.free_hit_after_no_ball}
        onChange={(free_hit_after_no_ball) => onChange({ ...value, free_hit_after_no_ball })}
      />
    </div>
  )
}

/**
 * Sport-specific match format settings (half length, overs limit, penalty
 * runs, ...) — `null` while unset, in which case the app just falls back to
 * its own defaults everywhere (see `lib/matchFormat`) without the match
 * having pinned specific values. Renders nothing for a sport with no format
 * modelled yet (only football and cricket, matching the backend union).
 *
 * The on/off toggle here is for *creating* a match, where "unset" is a real,
 * meaningful choice. There's no way to clear a format back to unset once
 * one exists (the update API only supports leaving it unchanged or setting
 * it, not clearing it) — editing an existing match's format uses
 * `FootballFields`/`CricketFields` directly instead, always against a
 * concrete value.
 */
export function MatchFormatEditor({
  sport,
  value,
  onChange,
}: {
  sport: MatchType
  value: MatchFormat | null
  onChange: (value: MatchFormat | null) => void
}) {
  if (sport !== 'football' && sport !== 'cricket') return null

  const enabled = value !== null

  return (
    <div>
      <ToggleRow
        label="Customize match format"
        checked={enabled}
        onChange={(on) => {
          if (!on) {
            onChange(null)
            return
          }
          onChange(
            sport === 'cricket'
              ? { sport: 'Cricket', ...DEFAULT_CRICKET_FORMAT }
              : { sport: 'Football', ...DEFAULT_FOOTBALL_FORMAT },
          )
        }}
      />
      {enabled && (
        <div className="mt-2 border-t pt-3">
          {value.sport === 'Cricket' ? (
            <CricketFields
              value={value}
              onChange={(v) => onChange({ sport: 'Cricket', ...v })}
            />
          ) : (
            <FootballFields
              value={value}
              onChange={(v) => onChange({ sport: 'Football', ...v })}
            />
          )}
        </div>
      )}
    </div>
  )
}
