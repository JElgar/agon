import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import './index.css'
import App from './App.tsx'
import { ThemeProvider } from '@/hooks/useTheme'
import { MatchCardPreview } from '@/components/agon/MatchCard.preview'
import { ProfilePreview } from '@/components/agon/Profile.preview'

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
    default:
      return <App />
  }
})()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>{root}</QueryClientProvider>
  </StrictMode>,
)
