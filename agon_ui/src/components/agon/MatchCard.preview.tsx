import type { components } from '@/types/api'
import { MatchCard } from './MatchCard'
import { ThemeToggle } from '@/components/ThemeToggle'

type Match = components['schemas']['Match']

/**
 * Dev-only visual preview of MatchCard with representative data (a tennis Sets
 * result and a football Simple result). Reach it at `/?preview=matchcard`.
 * Not part of the app; safe to delete once the feed page is wired.
 */

const tennisMatch: Match = {
  id: 'm-tennis',
  name: 'Riverside Tennis Club',
  description: '',
  match_type: 'tennis',
  status: 'completed',
  starts_at: '2026-06-28T17:00:00Z',
  header_photos: [],
  sides: [
    { id: 's-a', name: 'Sofia Lindqvist' },
    { id: 's-b', name: 'Alex Morgan' },
  ],
  players: [],
  confirmed_score: {
    winner_side_id: 's-a',
    score: {
      type: 'Sets',
      entries: [
        { side_id: 's-a', sets: [6, 6] },
        { side_id: 's-b', sets: [3, 2] },
      ],
    },
  },
  social: { like_count: 3, comment_count: 2, i_liked: false },
}

const footballMatch: Match = {
  id: 'm-football',
  name: 'Hackney Marshes',
  description: '',
  match_type: 'football',
  status: 'completed',
  starts_at: '2026-06-22T14:00:00Z',
  header_photos: [],
  sides: [
    { id: 'f-a', name: 'The Wanderers' },
    { id: 'f-b', name: 'Sunday FC' },
  ],
  players: [],
  pending_score: {
    submission_id: 'sub-1',
    winner_side_id: 'f-a',
    score: {
      type: 'Simple',
      entries: [
        { side_id: 'f-a', points: 3 },
        { side_id: 'f-b', points: 1 },
      ],
    },
    confirmations: [],
  },
  social: { like_count: 5, comment_count: 3, i_liked: true },
}

const scheduledMatch: Match = {
  id: 'm-upcoming',
  name: 'Club Ladder',
  description: '',
  match_type: 'squash',
  status: 'scheduled',
  starts_at: '2026-07-15T18:00:00Z',
  header_photos: [],
  sides: [
    { id: 'u-a', name: 'Priya Shah' },
    { id: 'u-b', name: 'Tom Brennan' },
  ],
  players: [],
  social: { like_count: 0, comment_count: 0, i_liked: false },
}

export function MatchCardPreview() {
  return (
    <div className="min-h-screen bg-background p-6">
      <div className="mx-auto flex max-w-md flex-col gap-4">
        <div className="flex items-center justify-between">
          <h1 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">
            MatchCard preview
          </h1>
          <ThemeToggle />
        </div>
        <MatchCard match={tennisMatch} onOpen={() => alert('open tennis')} />
        <MatchCard match={footballMatch} onOpen={() => alert('open football')} />
        <MatchCard match={scheduledMatch} onOpen={() => alert('open upcoming')} />
      </div>
    </div>
  )
}
