import type { components } from '@/types/api'
import {
  bowlingFigures,
  dismissalLabel,
  formatOvers,
  playerNameFor,
  runProgression,
  type CricketInnings,
} from '@/lib/cricketScore'
import { cricketFormat } from '@/lib/matchFormat'
import { CricketRunRateChart, type RunRateSeries } from './CricketRunRateChart'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

type Match = components['schemas']['Match']

const SERIES_COLORS = ['var(--color-chart-1)', 'var(--color-chart-2)']

function sideNameFor(match: Match, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
}

/**
 * A match's full cricket scorecard: a run-progression graph comparing every
 * ball-by-ball-scored innings, followed by each innings' batting/bowling
 * cards. Renders nothing if there's no per-innings detail to show (e.g. only
 * a headline score was ever recorded).
 */
export function CricketScorecard({ match, innings }: { match: Match; innings: CricketInnings[] }) {
  if (innings.length === 0) return null

  const format = cricketFormat(match.format)
  const series: RunRateSeries[] = innings
    .filter((inn) => inn.deliveries.length > 0)
    .map((inn, i) => ({
      key: `${inn.batting_side_id}-${i}`,
      label: sideNameFor(match, inn.batting_side_id),
      color: SERIES_COLORS[i % SERIES_COLORS.length],
      points: runProgression(inn.deliveries, format),
      finalRuns: inn.runs,
      finalWickets: inn.wickets,
    }))

  return (
    <div className="flex flex-col gap-4">
      {series.length > 0 && (
        <div className="rounded-xl border bg-card p-4">
          <p className="mb-3 text-sm font-medium">Run progression</p>
          <CricketRunRateChart series={series} />
        </div>
      )}

      {innings.map((inn, i) => (
        <InningsCard key={`${inn.batting_side_id}-${i}`} match={match} innings={inn} />
      ))}
    </div>
  )
}

function InningsCard({ match, innings }: { match: Match; innings: CricketInnings }) {
  if (innings.batting.length === 0 && innings.bowling.length === 0) return null

  const battingName = sideNameFor(match, innings.batting_side_id)
  const bowlingName = sideNameFor(match, innings.bowling_side_id)

  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      <div className="border-b px-4 py-3">
        <p className="text-sm font-medium">{battingName}</p>
        <p className="text-xs text-muted-foreground">
          {innings.runs}/{innings.wickets} ({formatOvers(innings.overs)} ov) vs {bowlingName}
        </p>
      </div>

      {innings.batting.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Batter</TableHead>
              <TableHead className="text-right">R</TableHead>
              <TableHead className="text-right">B</TableHead>
              <TableHead className="text-right">4s</TableHead>
              <TableHead className="text-right">6s</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {innings.batting.map((b) => (
              <TableRow key={b.player_id}>
                <TableCell>
                  <p className="font-medium">{playerNameFor(match, b.player_id)}</p>
                  <p className="text-[11px] text-muted-foreground">
                    {b.dismissal ? dismissalLabel(b.dismissal.kind) : 'not out'}
                  </p>
                </TableCell>
                <TableCell className="text-right tabular-nums">{b.runs}</TableCell>
                <TableCell className="text-right tabular-nums">{b.balls_faced}</TableCell>
                <TableCell className="text-right tabular-nums">{b.fours}</TableCell>
                <TableCell className="text-right tabular-nums">{b.sixes}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {innings.bowling.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Bowler</TableHead>
              <TableHead className="text-right">O-M-R-W</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {innings.bowling.map((bw) => (
              <TableRow key={bw.player_id}>
                <TableCell className="font-medium">{playerNameFor(match, bw.player_id)}</TableCell>
                <TableCell className="text-right tabular-nums">{bowlingFigures(bw)}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
