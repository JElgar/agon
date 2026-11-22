import { useNavigate, useParams } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { EditMatchDetailsFlow } from '@/components/agon/EditMatchDetailsFlow'
import { useMatchQuery } from '@/hooks/useMatch'

/**
 * `/matches/:matchId/edit` — editing a match's details as its own page
 * rather than a card inline on the match detail page. It used to be inline,
 * seeded straight from the already-loaded match; James found that confusing
 * once editing a series match could mean two different screens (just this
 * match / the whole series) that could land well below the fold with
 * nothing to show they'd opened — a dedicated page makes "did my tap do
 * something" unambiguous, since the page itself changes.
 */
export function EditMatchPage() {
  const { matchId } = useParams()
  const navigate = useNavigate()
  const query = useMatchQuery(matchId)

  if (query.isLoading) {
    return (
      <div className="mx-auto max-w-xl">
        <div className="h-64 animate-pulse rounded-2xl border bg-card" aria-hidden />
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

  const onDone = () => navigate(`/matches/${matchId}`)

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-4">
      <div className="flex items-center gap-1">
        <button
          type="button"
          onClick={onDone}
          aria-label="Back"
          className="flex h-11 items-center gap-1 rounded-full px-2 text-foreground hover:bg-muted"
        >
          <ChevronLeft className="size-[22px]" strokeWidth={2.2} />
        </button>
        <h1 className="font-display text-xl font-extrabold">Edit match</h1>
      </div>

      <EditMatchDetailsFlow match={query.data} onDone={onDone} />
    </div>
  )
}
