import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'
import { Search } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { sportLabel, type MatchType } from '@/lib/sports'

type MatchStatus = components['schemas']['MatchStatus']

/** Sports offered in the filter, same list/order as `BrowseMatchesPage`. */
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

const STATUSES: MatchStatus[] = [
  'scheduled',
  'in_progress',
  'completed',
  'cancelled',
]

function statusLabel(status: string): string {
  switch (status) {
    case 'in_progress':
      return 'In progress'
    case 'scheduled':
      return 'Scheduled'
    case 'completed':
      return 'Completed'
    case 'cancelled':
      return 'Cancelled'
    default:
      return status
  }
}

/** Debounce for the search input, same as `AdminUsersPage`. */
const DEBOUNCE_MS = 300

/**
 * Admin panel: view and filter every match (`GET /admin/matches`). Second
 * resource in the panel, following `AdminUsersPage`'s list shape — but with
 * no edit dialog: a row links straight through to the match's own page
 * instead of duplicating its detail here.
 */
export function AdminMatchesPage() {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')
  const [sport, setSport] = useState<MatchType | 'all'>('all')
  const [status, setStatus] = useState<MatchStatus | 'all'>('all')
  const [cursor, setCursor] = useState<string | undefined>(undefined)

  useEffect(() => {
    const id = setTimeout(() => {
      setDebounced(term.trim())
      setCursor(undefined)
    }, DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [term])

  useEffect(() => {
    setCursor(undefined)
  }, [sport, status])

  const query = useQuery({
    queryKey: ['admin-matches', debounced, sport, status, cursor],
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/admin/matches', {
        params: {
          query: {
            q: debounced || undefined,
            sport: sport === 'all' ? undefined : sport,
            status: status === 'all' ? undefined : status,
            cursor,
          },
        },
      })
      if (error || !data) throw new Error('Failed to list matches')
      return data
    },
  })

  const matches = query.data?.items ?? []

  const stats = useQuery({
    queryKey: ['admin-stats'],
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/admin/stats')
      if (error || !data) throw new Error('Failed to load stats')
      return data
    },
  })

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <div>
        <h1 className="font-display text-xl font-extrabold md:text-3xl">
          Matches
        </h1>
        <p className="text-sm text-muted-foreground">
          View and filter every match.
        </p>
      </div>

      <Card className="w-fit">
        <CardHeader className="pb-1">
          <CardTitle className="text-sm font-medium text-muted-foreground">
            Total matches
          </CardTitle>
        </CardHeader>
        <CardContent>
          <span className="font-display text-3xl font-extrabold">
            {stats.data ? stats.data.total_matches.toLocaleString() : '—'}
          </span>
        </CardContent>
      </Card>

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative max-w-sm flex-1">
          <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={term}
            onChange={(e) => setTerm(e.target.value)}
            placeholder="Search by match name…"
            className="pl-9"
          />
        </div>
        <select
          value={sport}
          onChange={(e) => setSport(e.target.value as MatchType | 'all')}
          className="h-9 w-40 rounded-md border bg-background px-3 text-sm"
        >
          <option value="all">All sports</option>
          {SPORTS.map((s) => (
            <option key={s} value={s}>
              {sportLabel(s)}
            </option>
          ))}
        </select>
        <select
          value={status}
          onChange={(e) => setStatus(e.target.value as MatchStatus | 'all')}
          className="h-9 w-40 rounded-md border bg-background px-3 text-sm"
        >
          <option value="all">All statuses</option>
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {statusLabel(s)}
            </option>
          ))}
        </select>
      </div>

      {query.isError && (
        <p className="py-8 text-center text-sm text-muted-foreground">
          Couldn't load matches.
        </p>
      )}

      {!query.isError && (
        <div className="overflow-hidden rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead className="hidden sm:table-cell">Sport</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="hidden sm:table-cell">Kick-off</TableHead>
                <TableHead />
              </TableRow>
            </TableHeader>
            <TableBody>
              {query.isLoading &&
                Array.from({ length: 5 }).map((_, i) => (
                  <TableRow key={i}>
                    <TableCell colSpan={5}>
                      <div className="h-4 w-full animate-pulse rounded bg-muted" />
                    </TableCell>
                  </TableRow>
                ))}
              {!query.isLoading && matches.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={5}
                    className="py-8 text-center text-muted-foreground"
                  >
                    No matches found.
                  </TableCell>
                </TableRow>
              )}
              {matches.map((match) => (
                <TableRow key={match.id}>
                  <TableCell className="max-w-[10rem] truncate font-medium sm:max-w-none">
                    {match.name}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    {sportLabel(match.sport as MatchType)}
                  </TableCell>
                  <TableCell className="text-muted-foreground">
                    {statusLabel(match.status)}
                  </TableCell>
                  <TableCell className="hidden text-muted-foreground sm:table-cell">
                    {new Date(match.starts_at).toLocaleString()}
                  </TableCell>
                  <TableCell>
                    <Button variant="outline" size="sm" asChild>
                      <Link to={`/matches/${match.id}`}>View</Link>
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <div className="flex items-center justify-between">
        <Button
          variant="outline"
          size="sm"
          disabled={!cursor}
          onClick={() => setCursor(undefined)}
        >
          Reset to first page
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!query.data?.next_cursor}
          onClick={() => setCursor(query.data?.next_cursor)}
        >
          Next page
        </Button>
      </div>
    </div>
  )
}
