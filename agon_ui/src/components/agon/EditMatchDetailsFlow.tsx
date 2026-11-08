import { useState } from 'react'
import { ChevronRight } from 'lucide-react'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { MatchDetailsEditor } from '@/components/agon/MatchDetailsEditor'
import { MatchSeriesDetailsEditor } from '@/components/agon/MatchSeriesDetailsEditor'

type Match = components['schemas']['Match']

/**
 * What tapping "Edit" on a match actually opens. A match outside any series
 * has only one thing it could mean, so it goes straight to
 * `MatchDetailsEditor`. A match that belongs to a series asks first — "just
 * this match" or "the whole series" — because the two edit different,
 * non-overlapping field sets (this occurrence's own date and side
 * names/colours vs. the series' shared name/description/location/photos
 * and kickoff time), so one combined form showing all of it and sorting out
 * scope afterwards was confusing about what actually applied where.
 */
export function EditMatchDetailsFlow({ match, onDone }: { match: Match; onDone: () => void }) {
  const [scope, setScope] = useState<'single' | 'series' | null>(match.series_id ? null : 'single')

  if (match.series_id && scope === null) {
    return (
      <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
        <p className="text-sm font-medium">This match is part of a recurring series. What do you want to edit?</p>
        <button
          type="button"
          onClick={() => setScope('single')}
          className="flex items-center gap-3 rounded-lg border p-3 text-left transition-colors hover:bg-accent/50"
        >
          <span className="flex-1">
            <span className="block text-sm font-semibold">Just this match</span>
            <span className="block text-xs text-muted-foreground">
              Name, description, date &amp; time, location, photos and side names/colours — for this
              occurrence only.
            </span>
          </span>
          <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
        </button>
        <button
          type="button"
          onClick={() => setScope('series')}
          className="flex items-center gap-3 rounded-lg border p-3 text-left transition-colors hover:bg-accent/50"
        >
          <span className="flex-1">
            <span className="block text-sm font-semibold">The whole series</span>
            <span className="block text-xs text-muted-foreground">
              Name, description, location, photos and kickoff time — applied to this and every future
              match in the series.
            </span>
          </span>
          <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
        </button>
        <Button size="sm" variant="outline" onClick={onDone} className="self-start">
          Cancel
        </Button>
      </div>
    )
  }

  if (scope === 'series') {
    return <MatchSeriesDetailsEditor match={match} onDone={onDone} onBack={() => setScope(null)} />
  }

  return (
    <MatchDetailsEditor match={match} onDone={onDone} onBack={match.series_id ? () => setScope(null) : undefined} />
  )
}
