import { useQuery } from '@tanstack/react-query'
import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Avatar } from '@/components/agon/Avatar'
import { SportBadge } from '@/components/agon/SportBadge'
import { StatusBadge } from '@/components/agon/StatusBadge'
import { ScoreConfirmationBar } from '@/components/agon/ScoreConfirmationBar'
import { useCurrentUserId } from '@/hooks/useCurrentUserId'
import { displayScore, headlineBySide, headlineLabel, setLine } from '@/lib/score'
import { memberName } from '@/lib/members'

type Match = components['schemas']['Match']
type MatchSide = components['schemas']['MatchSide']
type MatchPlayer = components['schemas']['MatchPlayer']

function sideName(side: MatchSide | undefined, fallback: string): string {
  return side?.name?.trim() || fallback
}

/** Full match view: score (with confirm/dispute when pending), sides + rosters. */
export function MatchDetailPage() {
  const { matchId } = useParams()
  const navigate = useNavigate()
  const currentUserId = useCurrentUserId()

  const query = useQuery({
    queryKey: ['match', matchId],
    enabled: !!matchId,
    queryFn: async (): Promise<Match> => {
      const { data, error } = await fetchClient.GET('/matches/{match_id}', {
        params: { path: { match_id: matchId! } },
      })
      if (error || !data) throw new Error('Failed to load match')
      return data
    },
  })

  if (query.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-xl border bg-card" aria-hidden />
      </div>
    )
  }

  if (query.isError || !query.data) {
    return (
      <div className="py-16 text-center">
        <p className="mb-4 text-muted-foreground">Couldn't load this match.</p>
        <Button variant="outline" onClick={() => query.refetch()}>
          Retry
        </Button>
      </div>
    )
  }

  const match = query.data
  const [sideA, sideB] = match.sides
  const nameA = sideName(sideA, 'Side A')
  const nameB = sideName(sideB, 'Side B')

  const scoreInfo = displayScore(match)
  const headline = scoreInfo ? headlineBySide(scoreInfo.score) : {}
  const sets = scoreInfo ? setLine(scoreInfo.score, match.sides) : []
  const aWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideA?.id
  const bWon = scoreInfo?.winnerSideId && scoreInfo.winnerSideId === sideB?.id

  return (
    <div className="mx-auto flex max-w-xl flex-col gap-4">
      <div className="flex items-center justify-between">
        <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
          <ChevronLeft className="size-4" /> Back
        </Button>
        <SportBadge sport={match.match_type} />
      </div>

      <div className="rounded-xl border bg-card p-4">
        <p className="text-sm text-muted-foreground">{match.name}</p>

        {/* Score header */}
        {scoreInfo ? (
          <div className="mt-3 flex items-center justify-between">
            <div className="flex-1">
              <p className={cn('text-sm', aWon && 'font-medium')}>{nameA}</p>
            </div>
            <div className="px-3 text-center">
              <div className="text-3xl font-medium tracking-tight">
                {headline[sideA?.id ?? ''] ?? 0}
                <span className="text-muted-foreground">–</span>
                {headline[sideB?.id ?? ''] ?? 0}
              </div>
              <div className="mt-0.5 text-[9px] uppercase tracking-widest text-muted-foreground">
                {headlineLabel(scoreInfo.score)}
              </div>
            </div>
            <div className="flex-1 text-right">
              <p className={cn('text-sm', bWon && 'font-medium')}>{nameB}</p>
            </div>
          </div>
        ) : (
          <p className="mt-3 text-sm text-muted-foreground">No score recorded yet.</p>
        )}

        {sets.length > 0 && (
          <div className="mt-2 border-t pt-2 text-center text-xs text-muted-foreground">
            {sets.map((s, i) => (
              <span key={i}>
                {i > 0 && <span className="mx-1.5 text-border">·</span>}
                Set {i + 1} <span className="font-medium text-foreground">{s}</span>
              </span>
            ))}
          </div>
        )}

        {scoreInfo && (
          <div className="mt-3">
            <StatusBadge
              status={scoreInfo.confirmed ? 'confirmed' : 'unconfirmed'}
            />
          </div>
        )}
      </div>

      {/* Confirm / dispute (only when the viewer's side owes a response) */}
      {match.pending_score && (
        <ScoreConfirmationBar
          match={match}
          currentUserId={currentUserId}
          variant="detail"
        />
      )}

      {/* Rosters, one column per side */}
      <div className="grid grid-cols-2 gap-3">
        <SideRoster
          title={nameA}
          players={match.players.filter((p) => p.side_id === sideA?.id)}
        />
        <SideRoster
          title={nameB}
          players={match.players.filter((p) => p.side_id === sideB?.id)}
        />
      </div>
    </div>
  )
}

function SideRoster({ title, players }: { title: string; players: MatchPlayer[] }) {
  return (
    <div className="rounded-xl border bg-card p-3">
      <p className="mb-2 truncate text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </p>
      <div className="flex flex-col gap-1.5">
        {players.length === 0 && (
          <p className="text-xs text-muted-foreground">No players.</p>
        )}
        {players.map((p, i) => {
          const name = memberName(p.member)
          const pending =
            p.member.invitation && p.member.invitation.status === 'pending'
          return (
            <div key={i} className="flex items-center gap-2">
              <Avatar name={name} size="md" />
              <span className="flex-1 truncate text-sm">{name}</span>
              {pending && (
                <span className="text-[10px] text-muted-foreground">invited</span>
              )}
            </div>
          )
        })}
      </div>
    </div>
  )
}
