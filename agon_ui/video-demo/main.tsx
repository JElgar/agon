import { createRoot } from 'react-dom/client'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router-dom'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MatchDetailPage } from '@/pages/MatchDetailPage'
import '@/index.css'

if (import.meta.env.MODE !== 'video-demo') throw new Error('Run Vite in video-demo mode.')

const root = document.getElementById('root')
if (!root) throw new Error('Missing demo root.')

createRoot(root).render(
  <QueryClientProvider client={new QueryClient()}>
    <main className="mx-auto min-h-screen max-w-5xl px-4 py-6">
    <BrowserRouter>
      <Routes>
        <Route path="/matches/:matchId" element={<MatchDetailPage />} />
        <Route path="*" element={<Navigate to="/matches/demo" replace />} />
      </Routes>
    </BrowserRouter>
    </main>
  </QueryClientProvider>,
)
