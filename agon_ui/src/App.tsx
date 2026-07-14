import { useEffect } from 'react'
import {
  BrowserRouter,
  Routes,
  Route,
  Navigate,
  useLocation,
} from 'react-router-dom'
import { AuthProvider, useAuth } from '@/hooks/useAuth'
import { LoginForm } from '@/components/auth/LoginForm'
import { CreateProfileForm } from '@/components/auth/CreateProfileForm'
import { InvitePreviewBanner } from '@/components/auth/InvitePreviewBanner'
import { AppSidebar } from '@/components/AppSidebar'
import { Button } from '@/components/ui/button'
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar'
import { ThemeProvider } from '@/hooks/useTheme'
import { useQuery } from '@tanstack/react-query'
import { fetchClient } from '@/lib/api-client'
import { FeedPage } from '@/pages/FeedPage'
import { ProfilePage } from '@/pages/ProfilePage'
import { LogMatchPage } from '@/pages/LogMatchPage'
import { MatchDetailPage } from '@/pages/MatchDetailPage'
import { NotificationsPage } from '@/pages/NotificationsPage'
import { UserSearchPage } from '@/pages/UserSearchPage'
import { FollowListPage } from '@/pages/FollowListPage'
import { AcceptInvitePage } from '@/pages/AcceptInvitePage'
import {
  getPendingInvite,
  setPendingInvite,
} from '@/lib/pendingInvite'

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

/** The signed-in chrome: a responsive sidebar (off-canvas on mobile) + routed
 *  content. A slim top bar carries the sidebar trigger on mobile. */
function AppShell({ email, onSignOut }: { email: string; onSignOut: () => void }) {
  return (
    <SidebarProvider>
      <AppSidebar email={email} onSignOut={onSignOut} />
      <SidebarInset>
        {/* Mobile top bar: hamburger to open the nav sheet. Hidden on desktop,
            where the sidebar is always visible. */}
        <header className="flex h-14 items-center gap-2 border-b px-4 md:hidden">
          <SidebarTrigger />
          <span className="text-lg font-bold">Agon</span>
        </header>

        <main className="container mx-auto px-4 py-8">
          <Routes>
          <Route path="/" element={<HomeRedirect />} />
          <Route path="/feed" element={<FeedPage />} />
          <Route path="/matches/new" element={<LogMatchPage />} />
          <Route path="/matches/:matchId" element={<MatchDetailPage />} />
          <Route path="/teams" element={<ComingSoon title="Teams" />} />
          <Route path="/teams/:teamId" element={<ComingSoon title="Team" />} />
          <Route path="/notifications" element={<NotificationsPage />} />
          <Route path="/invitations" element={<ComingSoon title="Invitations" />} />
          <Route path="/invite/:token" element={<AcceptInvitePage />} />
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
          <Route path="*" element={<HomeRedirect />} />
          </Routes>
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}

/**
 * Where "/" (and unknown paths) land. Normally the feed — but if the visitor
 * arrived via an invite link, the token was stashed before login (and survives
 * the OAuth redirect back to the app origin, which drops the path). Once signed
 * in with a profile, send them to accept it. AcceptInvitePage clears the token.
 */
function HomeRedirect() {
  const pending = getPendingInvite()
  return (
    <Navigate to={pending ? `/invite/${pending}` : '/feed'} replace />
  )
}

/** Extract an invite token from an `/invite/:token` pathname, else null. */
function inviteTokenFromPath(pathname: string): string | null {
  const match = pathname.match(/^\/invite\/([^/]+)/)
  return match ? decodeURIComponent(match[1]) : null
}

/**
 * Persist an invite token from `/invite/:token` the moment it's seen, before
 * the auth gate can swallow the route. Reads the pathname directly (not
 * `useParams`) so it fires even on the logged-out screen, which renders no
 * `<Routes>`. Lets the token survive login — including OAuth, which redirects
 * back to the app origin and loses the path.
 */
function useInviteCapture(): string | null {
  const location = useLocation()
  const token = inviteTokenFromPath(location.pathname)
  useEffect(() => {
    if (token) setPendingInvite(token)
  }, [token])
  return token
}

/** Whether the signed-in Supabase account has completed Agon signup. */
type ProfileGate = 'has-profile' | 'needs-profile'

function AuthenticatedApp() {
  const { user, session, signOut, loading: authLoading } = useAuth()

  // Capture an invite token (if the visitor arrived via a link) so it survives
  // login. `inviteToken` is the token on the *current* URL, used to preview the
  // invite on the logged-out screen.
  const inviteToken = useInviteCapture()

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
          {inviteToken && <InvitePreviewBanner token={inviteToken} />}
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
        <div className="w-full max-w-md">
          {inviteToken && <InvitePreviewBanner token={inviteToken} />}
          <CreateProfileForm
            email={user.email || ''}
            onProfileCreated={() => gate.refetch()}
          />
        </div>
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
