/**
 * Dev-only fetch stub for component previews. Must be imported BEFORE any module
 * that creates the openapi-fetch client (`@/lib/api-client`), because that client
 * captures `globalThis.fetch` once at creation — so the override has to be in
 * place first. Import this as the very first line of `main.tsx`.
 *
 * Only active when `?preview=feed` is set; otherwise it's a no-op and the real
 * fetch is used untouched. Throwaway, alongside the *.preview.tsx files.
 */
import type { components } from '@/types/api'

type FeedPageData = components['schemas']['FeedPage']

const page1: FeedPageData = {
  next_cursor: 'cursor-2',
  items: [
    {
      type: 'Match',
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
    },
    {
      type: 'Match',
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
    },
  ],
}

const page2: FeedPageData = {
  items: [
    {
      type: 'Match',
      id: 'm-squash',
      name: 'Club Ladder',
      description: '',
      match_type: 'squash',
      status: 'completed',
      starts_at: '2026-06-20T18:00:00Z',
      header_photos: [],
      sides: [
        { id: 'q-a', name: 'Priya Shah' },
        { id: 'q-b', name: 'Tom Brennan' },
      ],
      players: [],
      confirmed_score: {
        winner_side_id: 'q-b',
        score: {
          type: 'Sets',
          entries: [
            { side_id: 'q-a', sets: [11, 9, 7] },
            { side_id: 'q-b', sets: [9, 11, 11] },
          ],
        },
      },
      social: { like_count: 1, comment_count: 0, i_liked: false },
    },
  ],
}

if (
  typeof window !== 'undefined' &&
  new URLSearchParams(window.location.search).get('preview') === 'feed'
) {
  const real = window.fetch.bind(window)
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === 'string' ? input : input.toString()
    if (url.includes('/feed')) {
      const cursor = new URL(url, window.location.origin).searchParams.get('cursor')
      const body = cursor === 'cursor-2' ? page2 : page1
      return new Response(JSON.stringify(body), {
        status: 200,
        headers: { 'Content-Type': 'application/json; charset=utf-8' },
      })
    }
    return real(input, init)
  }
}
