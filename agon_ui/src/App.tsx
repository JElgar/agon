import { BrowserRouter, Routes, Route, Navigate, Link, useLocation } from 'react-router-dom'
import { AuthProvider, useAuth } from '@/hooks/useAuth'
import { LoginForm } from '@/components/auth/LoginForm'
import { CreateProfileForm } from '@/components/auth/CreateProfileForm'
import { Button } from '@/components/ui/button'
import { ThemeProvider } from '@/hooks/useTheme'
import { ThemeToggle } from '@/components/ThemeToggle'
import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { FeedPage } from '@/pages/FeedPage'
import { ProfilePage } from '@/pages/ProfilePage'
import { LogMatchPage } from '@/pages/LogMatchPage'
import { MatchDetailPage } from '@/pages/MatchDetailPage'
import { NotificationsPage } from '@/pages/NotificationsPage'
import { UserSearchPage } from '@/pages/UserSearchPage'
import { FollowListPage } from '@/pages/FollowListPage'

function NavLink({ to, children }: { to: string; children: React.ReactNode }) {
  const location = useLocation()
  const isActive =
    location.pathname === to || (to !== '/' && location.pathname.startsWith(to))

  return (
    <Link
      to={to}
      className={`px-3 py-2 rounded-md text-sm font-medium transition-colors ${
        isActive
          ? 'bg-primary/10 text-primary'
          : 'text-muted-foreground hover:text-foreground hover:bg-muted'
      }`}
    >
      {children}
    </Link>
  )
}

/** Full-screen centered content, used for the loading / error / auth gates. */
function CenteredMessage({ children }: { children: React.ReactNode }) {
  return (
    <div className="min-h-screen flex items-center justify-center">{children}</div>
  )
}

/** Placeholder for a page not yet built in the rewrite. */
function ComingSoon({ title }: { title: string }) {
  return (
    <div className="text-center py-16">
      <h2 className="text-xl font-semibold mb-2">{title}</h2>
      <p className="text-muted-foreground">This page is being rebuilt.</p>
    </div>
  )
}

/** The signed-in chrome: header nav + routed content. */
function AppShell({ email, onSignOut }: { email: string; onSignOut: () => void }) {
  return (
    <div className="min-h-screen bg-background">
      <header className="border-b">
        <div className="container mx-auto px-4 py-4 flex justify-between items-center">
          <div className="flex items-center space-x-8">
            <h1 className="text-2xl font-bold">Agon</h1>
            <nav className="flex space-x-2">
              <NavLink to="/feed">Feed</NavLink>
              <NavLink to="/search">Find people</NavLink>
              <NavLink to="/teams">Teams</NavLink>
              <NavLink to="/notifications">Notifications</NavLink>
              <NavLink to="/profile">Profile</NavLink>
            </nav>
          </div>
          <div className="flex items-center space-x-4">
            <span className="text-sm text-muted-foreground">{email}</span>
            <ThemeToggle />
            <Button onClick={onSignOut} variant="outline">
              Sign Out
            </Button>
          </div>
        </div>
      </header>

      <main className="container mx-auto px-4 py-8">
        <Routes>
          <Route path="/" element={<Navigate to="/feed" replace />} />
          <Route path="/feed" element={<FeedPage />} />
          <Route path="/matches/new" element={<LogMatchPage />} />
          <Route path="/matches/:matchId" element={<MatchDetailPage />} />
          <Route path="/teams" element={<ComingSoon title="Teams" />} />
          <Route path="/teams/:teamId" element={<ComingSoon title="Team" />} />
          <Route path="/notifications" element={<NotificationsPage />} />
          <Route path="/invitations" element={<ComingSoon title="Invitations" />} />
          <Route path="/search" element={<UserSearchPage />} />
          <Route path="/profile" element={<ProfilePage />} />
          <Route path="/users/:userId" element={<ProfilePage />} />
          <Route
            path="/users/:userId/followers"
            element={<FollowListPage mode="followers" />}
          />
          <Route
            path="/users/:userId/following"
            element={<FollowListPage mode="following" />}
          />
          <Route path="*" element={<Navigate to="/feed" replace />} />
        </Routes>
      </main>
    </div>
  )
}

/** Whether the signed-in Supabase account has completed Agon signup. */
type ProfileGate = 'has-profile' | 'needs-profile'

function AuthenticatedApp() {
  const { user, session, signOut, loading: authLoading } = useAuth()

  // Resolve the caller's Agon profile. A 404 means the Supabase account exists
  // but hasn't completed signup (no `/users` record yet) → show profile creation.
  // We use the raw fetch client here (not $api) because the gate decision hinges
  // on the HTTP *status*, which openapi-react-query hides behind the parsed body.
  const gate = useQuery({
    queryKey: ['profile-gate'],
    enabled: !!session,
    retry: false,
    queryFn: async (): Promise<ProfileGate> => {
      const { data, response } = await fetchClient.GET('/users/me')
      if (data) return 'has-profile'
      if (response.status === 404) return 'needs-profile'
      throw new Error(`Failed to load profile (${response.status})`)
    },
  })

  if (authLoading) {
    return <CenteredMessage>Loading…</CenteredMessage>
  }

  if (!user) {
    return (
      <CenteredMessage>
        <div className="w-full max-w-md">
          <h1 className="text-3xl font-bold text-center mb-8">Welcome to Agon</h1>
          <LoginForm />
        </div>
      </CenteredMessage>
    )
  }

  if (gate.isLoading) {
    return <CenteredMessage>Checking your profile…</CenteredMessage>
  }

  if (gate.data === 'needs-profile') {
    return (
      <CenteredMessage>
        <CreateProfileForm
          email={user.email || ''}
          onProfileCreated={() => gate.refetch()}
        />
      </CenteredMessage>
    )
  }

  if (gate.isError) {
    return (
      <CenteredMessage>
        <div className="text-center">
          <h2 className="text-xl font-semibold mb-4">Something went wrong</h2>
          <p className="text-muted-foreground mb-4">Couldn't load your profile.</p>
          <div className="space-x-2">
            <Button onClick={() => gate.refetch()} variant="outline">
              Retry
            </Button>
            <Button onClick={signOut} variant="outline">
              Sign Out
            </Button>
          </div>
        </div>
      </CenteredMessage>
    )
  }

  return <AppShell email={user.email || ''} onSignOut={signOut} />
}

function App() {
  return (
    <ThemeProvider defaultTheme="system" storageKey="agon-ui-theme">
      <BrowserRouter>
        <AuthProvider>
          <AuthenticatedApp />
        </AuthProvider>
      </BrowserRouter>
    </ThemeProvider>
  )
}

export default App
