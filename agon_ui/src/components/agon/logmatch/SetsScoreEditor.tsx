import { Trophy } from 'lucide-react'
import { CARD } from './parts'
import { cn } from '@/lib/utils'
import { playedSets, type SetRow } from '@/lib/logMatch'

/**
 * Set-by-set score entry: one row per side, one column per set, the
 * winning game count of each set emphasised, and the overall result shown
 * underneath once one side is ahead on sets.
 */
export function SetsScoreEditor({
  sideAName,
  sideBName,
  rows,
  onChange,
}: {
  sideAName: string
  sideBName: string
  rows: SetRow[]
  onChange: (rows: SetRow[]) => void
}) {
  const sets = playedSets(rows)
  const aSets = sets.filter((s) => s.a > s.b).length
  const bSets = sets.filter((s) => s.b > s.a).length
  const columns = `minmax(88px, 1fr) repeat(${rows.length}, 52px)`

  const cell = (i: number, key: 'a' | 'b', label: string) => {
    const r = rows[i]
    const mine = Number(r[key])
    const theirs = Number(r[key === 'a' ? 'b' : 'a'])
    const won = r[key] !== '' && mine > theirs
    return (
      <input
        key={`${key}${i}`}
        type="number"
        min={0}
        inputMode="numeric"
        value={r[key]}
        placeholder="0"
        aria-label={`${label} set ${i + 1}`}
        onChange={(e) => onChange(rows.map((row, j) => (j === i ? { ...row, [key]: e.target.value } : row)))}
        className={cn(
          'h-12 w-full rounded-xl border bg-card text-center [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none font-display text-lg font-extrabold outline-none focus-visible:ring-2 focus-visible:ring-ring',
          won ? 'border-foreground text-foreground' : 'text-muted-foreground',
        )}
      />
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <div className={CARD}>
        <div className="overflow-x-auto">
          <div className="grid items-center gap-2" style={{ gridTemplateColumns: columns }}>
            <span />
            {rows.map((_, i) => (
              <span key={i} className="text-center text-[11px] font-bold uppercase tracking-wider text-muted-foreground">
                Set {i + 1}
              </span>
            ))}
            <span className="text-sm font-bold leading-tight">{sideAName}</span>
            {rows.map((_, i) => cell(i, 'a', sideAName))}
            <span className="text-sm font-bold leading-tight">{sideBName}</span>
            {rows.map((_, i) => cell(i, 'b', sideBName))}
          </div>
        </div>
        <div className="flex gap-1">
          <button
            type="button"
            onClick={() => onChange([...rows, { a: '', b: '' }])}
            className="h-10 rounded-lg px-3 text-sm font-bold text-primary hover:bg-accent"
          >
            + Add set
          </button>
          {rows.length > 1 && (
            <button
              type="button"
              onClick={() => onChange(rows.slice(0, -1))}
              className="h-10 rounded-lg px-3 text-sm font-semibold text-muted-foreground hover:bg-accent"
            >
              Remove set
            </button>
          )}
        </div>
      </div>

      {aSets !== bSets && (
        <div className="flex items-center gap-3 rounded-2xl bg-primary px-4 py-3.5 text-primary-foreground">
          <Trophy className="size-5 shrink-0" />
          <span className="text-[15px] font-bold">
            {aSets > bSets ? sideAName : sideBName} win {Math.max(aSets, bSets)}–{Math.min(aSets, bSets)}
          </span>
        </div>
      )}
    </div>
  )
}
