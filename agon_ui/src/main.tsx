// Dev-only preview fetch stub — MUST be first so it overrides globalThis.fetch
// before @/lib/api-client (imported transitively by App) captures it. No-op
// unless a ?preview= param opts in.
import './pages/preview-fetch-stub'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from '@/hooks/useTheme'
import { MatchCardPreview } from '@/components/agon/MatchCard.preview'
import { ProfilePreview } from '@/components/agon/Profile.preview'
import { FeedPagePreview } from '@/pages/FeedPage.preview'

const queryClient = new QueryClient()

// Dev-only component preview gate: `/?preview=matchcard` renders a component in
// isolation (no auth/router), so new UI blocks can be eyeballed before pages are
// wired. Remove once a proper story/preview setup exists.
const preview = new URLSearchParams(window.location.search).get('preview')

const root = (() => {
  switch (preview) {
    case 'matchcard':
      return (
        <ThemeProvider defaultTheme="system" storageKey="agon-ui-theme">
          <MatchCardPreview />
        </ThemeProvider>
      )
    case 'profile':
      return (
        <ThemeProvider defaultTheme="system" storageKey="agon-ui-theme">
          <ProfilePreview />
        </ThemeProvider>
      )
    case 'feed':
      return (
        <ThemeProvider defaultTheme="system" storageKey="agon-ui-theme">
          <FeedPagePreview />
        </ThemeProvider>
      )
    default:
      return <App />
  }
})()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>{root}</QueryClientProvider>
  </StrictMode>,
)
