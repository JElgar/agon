import type { components } from '@/types/api'
import { formatHalfRounders, outKindLabel, playerNameFor } from '@/lib/roundersScore'
import type { ScorePlayers } from '@/lib/members'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

type Match = components['schemas']['Match']
type RoundersScoreInnings = components['schemas']['RoundersScoreInnings']
type RoundersBattingEntry = components['schemas']['RoundersBattingEntry']
type RoundersBowlingEntry = components['schemas']['RoundersBowlingEntry']

function sideNameFor(match: Match, sideId: string): string {
  return match.sides.find((s) => s.id === sideId)?.name?.trim() || 'This side'
}

/**
 * A match's full rounders scorecard: each innings' batting/bowling cards.
 * Mirrors `CricketScorecard`, minus the run-progression graph — rounders has
 * no equivalent continuous metric (a batter's turn is one or a few balls,
 * not an over-by-over rate) to plot one against. Renders nothing if there's
 * no per-innings detail to show (e.g. only a headline score was recorded).
 *
 * `match` is always the full detail-page `Match` here (own roster included),
 * so `players` (the score's resolved-name map) is a redundant-but-cheap
 * first lookup rather than the only source a feed card relies on — see
 * `playerNameFor`.
 */
export function RoundersScorecard({
  match,
  innings,
  players,
}: {
  match: Match
  innings: RoundersScoreInnings[]
  players?: ScorePlayers
}) {
  if (innings.length === 0) return null

  return (
    <div className="flex flex-col gap-4">
      {innings.map((inn, i) => (
        <InningsCard key={`${inn.batting_side_id}-${i}`} match={match} innings={inn} players={players} />
      ))}
    </div>
  )
}

function InningsCard({
  match,
  innings,
  players,
}: {
  match: Match
  innings: RoundersScoreInnings
  players?: ScorePlayers
}) {
  const batting = innings.batting ?? []
  const bowling = innings.bowling ?? []
  if (batting.length === 0 && bowling.length === 0) return null

  const battingName = sideNameFor(match, innings.batting_side_id)
  const fieldingName = sideNameFor(match, innings.fielding_side_id)

  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      <div className="border-b px-4 py-3">
        <p className="text-sm font-medium">{battingName}</p>
        <p className="text-xs text-muted-foreground">
          {formatHalfRounders(innings.half_rounders)} rounders, {innings.outs} out (
          {innings.good_balls_bowled} good balls) vs {fieldingName}
        </p>
      </div>

      {batting.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Batter</TableHead>
              <TableHead className="w-16 text-right">½R</TableHead>
              <TableHead className="w-12 text-right">B</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {batting.map((b: RoundersBattingEntry) => (
              <TableRow key={b.player_id}>
                <TableCell>
                  <p className="font-medium">{playerNameFor(match, b.player_id, players) ?? '—'}</p>
                  <p className="text-[11px] text-muted-foreground">
                    {b.out ? outKindLabel(b.out.kind) : 'not out'}
                  </p>
                </TableCell>
                <TableCell className="w-16 text-right tabular-nums">
                  {formatHalfRounders(b.half_rounders)}
                </TableCell>
                <TableCell className="w-12 text-right tabular-nums">{b.balls_faced}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}

      {bowling.length > 0 && (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Bowler</TableHead>
              <TableHead className="w-12 text-right">B</TableHead>
              <TableHead className="w-12 text-right">NB</TableHead>
              <TableHead className="w-16 text-right">½R</TableHead>
              <TableHead className="w-12 text-right">O</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {bowling.map((bw: RoundersBowlingEntry) => (
              <TableRow key={bw.player_id}>
                <TableCell className="font-medium">{playerNameFor(match, bw.player_id, players) ?? '—'}</TableCell>
                <TableCell className="w-12 text-right tabular-nums">{bw.balls_bowled}</TableCell>
                <TableCell className="w-12 text-right tabular-nums">{bw.no_balls}</TableCell>
                <TableCell className="w-16 text-right tabular-nums">
                  {formatHalfRounders(bw.half_rounders_conceded)}
                </TableCell>
                <TableCell className="w-12 text-right tabular-nums">{bw.outs}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
