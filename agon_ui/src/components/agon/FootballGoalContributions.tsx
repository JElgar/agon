import { useMemo, useState } from 'react'
import {
  flexRender,
  getCoreRowModel,
  useReactTable,
  type ColumnDef,
} from '@tanstack/react-table'
import { ArrowDown } from 'lucide-react'
import type { components } from '@/types/api'
import {
  goalContributions,
  sortGoalContributions,
  type GoalContributionEntry,
  type GoalContributionSort,
  type FootballGoalEvent,
} from '@/lib/liveScore'
import type { ScorePlayers } from '@/lib/members'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

type Match = components['schemas']['Match']
type FeedMatch = components['schemas']['FeedMatch']
type SearchMatch = components['schemas']['SearchMatch']

declare module '@tanstack/react-table' {
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  interface ColumnMeta<TData, TValue> {
    /** Right-aligns the header/cell — used for the numeric goals/assists columns. */
    align?: 'right'
  }
}

/** A clickable column header for the sortable columns (goals/assists) — an
 *  `ArrowDown` marks whichever one is currently the table's primary sort key,
 *  matching the always-descending contract of `sortGoalContributions`. */
function SortableHeader({
  label,
  sortKey,
  activeSort,
  onSort,
}: {
  label: string
  sortKey: GoalContributionSort
  activeSort: GoalContributionSort
  onSort: (key: GoalContributionSort) => void
}) {
  return (
    <Button
      variant="ghost"
      size="sm"
      className="-mr-2 h-7 gap-1 px-2 text-xs"
      aria-pressed={activeSort === sortKey}
      onClick={() => onSort(sortKey)}
    >
      {label}
      <ArrowDown className={cn('size-3 text-muted-foreground', activeSort === sortKey && 'text-foreground')} />
    </Button>
  )
}

/**
 * A football match's goal-contributions table — every scorer and assister,
 * one row per player, goals and assists side by side. A shadcn/tanstack
 * data table: defaults to most goals first (assists breaking ties);
 * clicking the Goals/Assists column header flips the primary sort key
 * (see `sortGoalContributions` — always descending, no ascending toggle,
 * so the header's arrow just marks which column currently leads).
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

  const columns = useMemo<ColumnDef<GoalContributionEntry>[]>(
    () => [
      {
        accessorKey: 'name',
        header: 'Player',
        cell: ({ getValue }) => <span className="font-medium">{getValue<string>()}</span>,
      },
      {
        accessorKey: 'goals',
        header: () => <SortableHeader label="Goals" sortKey="goals" activeSort={sortBy} onSort={setSortBy} />,
        cell: ({ getValue }) => <span className="tabular-nums">{getValue<number>()}</span>,
        meta: { align: 'right' },
      },
      {
        accessorKey: 'assists',
        header: () => <SortableHeader label="Assists" sortKey="assists" activeSort={sortBy} onSort={setSortBy} />,
        cell: ({ getValue }) => <span className="tabular-nums">{getValue<number>()}</span>,
        meta: { align: 'right' },
      },
    ],
    [sortBy],
  )

  const table = useReactTable({
    data: entries,
    columns,
    getCoreRowModel: getCoreRowModel(),
    getRowId: (row) => row.key,
  })

  if (entries.length === 0) return null

  return (
    <div className={cn('rounded-xl border bg-card p-4', className)}>
      <p className="mb-3 text-sm font-medium">Goal contributions</p>
      <Table>
        <TableHeader>
          {table.getHeaderGroups().map((headerGroup) => (
            <TableRow key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <TableHead
                  key={header.id}
                  className={cn(
                    header.column.columnDef.meta?.align === 'right' && 'text-right',
                  )}
                >
                  {flexRender(header.column.columnDef.header, header.getContext())}
                </TableHead>
              ))}
            </TableRow>
          ))}
        </TableHeader>
        <TableBody>
          {table.getRowModel().rows.map((row) => (
            <TableRow key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <TableCell
                  key={cell.id}
                  className={cn(cell.column.columnDef.meta?.align === 'right' && 'text-right')}
                >
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
