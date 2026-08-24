import { useState } from 'react'
import type { components } from '@/types/api'
import {
  goalContributions,
  sortGoalContributions,
  type FootballGoalEvent,
  type GoalContributionSort,
} from '@/lib/liveScore'
import type { ScorePlayers } from '@/lib/members'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

const SORT_OPTIONS: { key: GoalContributionSort; label: string }[] = [
  { key: 'goals', label: 'Goals' },
  { key: 'assists', label: 'Assists' },
]

/**
 * A football match's goal-contributions table — every scorer and assister,
 * one row per player, goals and assists side by side. Defaults to most
 * goals first (assists breaking ties); the header buttons flip it to most
 * assists first (goals breaking ties instead) — see `sortGoalContributions`.
 * Fed by the same goal log as `FootballScorecard`'s event timeline
 * (live or finished, `own_goal` excluded from the scorer's own tally, same
 * as the backend's own accounting), so it stays in sync with it. Renders
 * nothing if nobody has scored or assisted yet.
 */
export function FootballGoalContributions({
  goals,
  match,
  players,
  className,
}: {
  goals: FootballGoalEvent[]
  match: Match | FeedMatch | SearchMatch
  players?: ScorePlayers
  className?: string
}) {
  const [sortBy, setSortBy] = useState<GoalContributionSort>('goals')
  const entries = sortGoalContributions(goalContributions(goals, match, players), sortBy)

  if (entries.length === 0) return null

  return (
    <div className={cn('rounded-xl border bg-card p-4', className)}>
      <div className="mb-3 flex items-center justify-between">
        <p className="text-sm font-medium">Goal contributions</p>
        <div className="flex gap-1">
          {SORT_OPTIONS.map((opt) => (
            <Button
              key={opt.key}
              variant={sortBy === opt.key ? 'secondary' : 'ghost'}
              size="sm"
              className="h-7 px-2 text-xs"
              aria-pressed={sortBy === opt.key}
              onClick={() => setSortBy(opt.key)}
            >
              {opt.label}
            </Button>
          ))}
        </div>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Player</TableHead>
            <TableHead className="w-12 text-right">G</TableHead>
            <TableHead className="w-12 text-right">A</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {entries.map((e) => (
            <TableRow key={e.key}>
              <TableCell className="font-medium">{e.name}</TableCell>
              <TableCell className="w-12 text-right tabular-nums">{e.goals}</TableCell>
              <TableCell className="w-12 text-right tabular-nums">{e.assists}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
