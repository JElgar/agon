import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import { FeedPage } from './FeedPage'
import { ThemeToggle } from '@/components/ThemeToggle'

/**
 * Dev-only preview of the wired FeedPage. The `/feed` endpoint is stubbed in
 * `preview-fetch-stub.ts` (imported first in main.tsx) with two pages of
 * real-shaped data, so the query → flatten → MatchCard → "Load more" path runs
 * without auth or a backend. Reach it at `/?preview=feed`. Throwaway.
 */
export function FeedPagePreview() {
  const queryClient = new QueryClient()
  return (
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <div className="min-h-screen bg-background p-6">
          <div className="mx-auto flex max-w-xl flex-col gap-4">
            <div className="flex items-center justify-between">
              <h1 className="text-sm font-medium uppercase tracking-wider text-muted-foreground">
                Feed page preview
              </h1>
              <ThemeToggle />
            </div>
            <FeedPage />
          </div>
        </div>
      </MemoryRouter>
    </QueryClientProvider>
  )
}
