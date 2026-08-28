import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
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
import { Avatar } from '@/components/agon/Avatar'
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

/** A clickable column header for the sortable columns (goals/assists).
 *  `sortGoalContributions` always sorts its active column descending — there's
 *  no ascending toggle to flip to — so instead of rotating an arrow, the
 *  active column is the only one showing an `ArrowDown` at all, and its
 *  label is bolded; the inactive column is a plain, arrow-less button.
 *  Clicking either one always just makes it the active (descending) sort. */
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
  const active = activeSort === sortKey
  return (
    <Button
      variant="ghost"
      size="sm"
      className="-mr-2 h-7 gap-1 px-2 text-xs"
      aria-pressed={active}
      onClick={() => onSort(sortKey)}
    >
      <span className={cn(active && 'font-semibold text-foreground')}>{label}</span>
      {active && <ArrowDown className="size-3" />}
    </Button>
  )
}

/**
 * A football match's goal-contributions table — every scorer and assister,
 * one row per player, goals and assists side by side. A shadcn/tanstack
 * data table: defaults to most goals first (assists breaking ties);
 * clicking the Goals/Assists column header makes it the primary sort key
 * (see `sortGoalContributions` and `SortableHeader`'s doc comments — always
 * descending, no ascending toggle, so only the active column shows an
 * arrow at all). Each row's player name is an avatar + link to their
 * profile, same as the match's own player-list rows (`SideRoster`) — plain
 * text for an external player with no linked account. Fed by the same goal
 * log as `FootballScorecard`'s event timeline (live or finished, `own_goal`
 * excluded from the scorer's own tally, same as the backend's own
 * accounting), so it stays in sync with it. Renders nothing if nobody has
 * scored or assisted yet.
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
  // Memoized: `useReactTable` keys its internal state off referential
  // equality of `data` (and `columns` below) — a fresh array every render
  // sends it into a re-render loop that hangs the tab.
  const entries = useMemo(
    () => sortGoalContributions(goalContributions(goals, match, players), sortBy),
    [goals, match, players, sortBy],
  )

  const columns = useMemo<ColumnDef<GoalContributionEntry>[]>(
    () => [
      {
        accessorKey: 'name',
        header: 'Player',
        cell: ({ row }) => {
          const { name, userId, avatarUrl } = row.original
          const content = (
            <>
              <Avatar name={name} imageUrl={avatarUrl} size="sm" />
              <span className="truncate font-medium">{name}</span>
            </>
          )
          // Only a linked Agon user has a profile to open — an external
          // player (or an id we couldn't resolve to one) stays plain text,
          // same as the match's own player-list rows (`SideRoster`).
          return userId ? (
            <Link to={`/users/${userId}`} className="flex min-w-0 items-center gap-2">
              {content}
            </Link>
          ) : (
            <div className="flex min-w-0 items-center gap-2">{content}</div>
          )
        },
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
