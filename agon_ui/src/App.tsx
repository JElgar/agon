import { useEffect } from 'react'
import { BrowserRouter, Routes, Route, Navigate, Link, useLocation } from 'react-router-dom'
import { AuthProvider, useAuth } from '@/hooks/useAuth'
import { LoginForm } from '@/components/auth/LoginForm'
import { CreateProfileForm } from '@/components/auth/CreateProfileForm'
import { TeamsPage } from '@/components/TeamsPage'
import { TeamDetailsPage } from '@/components/TeamDetailsPage'
import { GamesPage } from '@/components/GamesPage'
import { CreateGamePage } from '@/components/CreateGamePage'
import { GameDetailsPage } from '@/components/GameDetailsPage'
import { Button } from '@/components/ui/button'
import { useUserProfile } from '@/hooks/useUserProfile'
import { useGetTeams } from '@/hooks/useApi'
import { ThemeProvider } from '@/hooks/useTheme'
import { ThemeToggle } from '@/components/ThemeToggle'

function NavLink({ to, children }: { to: string; children: React.ReactNode }) {
  const location = useLocation()
  const isActive = location.pathname === to || (to !== '/' && location.pathname.startsWith(to))
  
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

function AuthenticatedApp() {
  const { user, signOut, loading: authLoading } = useAuth()
  const { hasProfile, loading: profileLoading, error: profileError, checkUserProfile, refreshProfile } = useUserProfile()
  const { data: teams, loading: teamsLoading, getTeams } = useGetTeams()

  // Check user profile when user is authenticated
  useEffect(() => {
    console.log('App effect - user:', !!user, 'hasProfile:', hasProfile)
    if (user && hasProfile === null) {
      console.log('Triggering profile check...')
      checkUserProfile()
    }
  }, [user, hasProfile, checkUserProfile])

  // Load teams when user has profile
  useEffect(() => {
    if (hasProfile === true) {
      getTeams()
    }
  }, [hasProfile, getTeams])

  if (authLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div>Loading...</div>
      </div>
    )
  }

  if (!user) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="w-full max-w-md">
          <h1 className="text-3xl font-bold text-center mb-8">Welcome to Agon</h1>
          {!import.meta.env.VITE_SUPABASE_URL && (
            <div className="mb-6 p-4 bg-yellow-50 border border-yellow-200 rounded-md">
              <p className="text-sm text-yellow-800">
                <strong>Development Mode:</strong> Configure Supabase credentials in your .env file to enable authentication.
              </p>
            </div>
          )}
          <LoginForm />
        </div>
      </div>
    )
  }

  // Show loading while checking profile
  if (profileLoading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div>Checking your profile...</div>
      </div>
    )
  }

  // Show profile creation form if user doesn't have a profile
  if (hasProfile === false) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background">
        <CreateProfileForm 
          email={user.email || ''} 
          onProfileCreated={() => {
            // Directly check profile again after creation
            checkUserProfile()
          }} 
        />
      </div>
    )
  }

  // Show error state if there was an issue checking profile
  if (profileError) {
    return (
      <div className="min-h-screen flex items-center justify-center bg-background">
        <div className="text-center">
          <h2 className="text-xl font-semibold mb-4">Something went wrong</h2>
          <p className="text-muted-foreground mb-4">{profileError}</p>
          <div className="space-x-2">
            <Button onClick={() => checkUserProfile()} variant="outline">
              Retry
            </Button>
            <Button onClick={signOut} variant="outline">
              Sign Out
            </Button>
          </div>
        </div>
      </div>
    )
  }

  // Debug info (temporary)
  console.log('Current state - hasProfile:', hasProfile, 'profileLoading:', profileLoading, 'user:', !!user)

  return (
    <div className="min-h-screen bg-background">
      <header className="border-b">
        <div className="container mx-auto px-4 py-4 flex justify-between items-center">
          <div className="flex items-center space-x-8">
            <h1 className="text-2xl font-bold">Agon</h1>
            <nav className="flex space-x-6">
              <NavLink to="/teams">Teams</NavLink>
              <NavLink to="/games">Games</NavLink>
            </nav>
          </div>
          <div className="flex items-center space-x-4">
            <span className="text-sm text-muted-foreground">
              {user.email}
            </span>
            <ThemeToggle />
            <Button onClick={signOut} variant="outline">
              Sign Out
            </Button>
          </div>
        </div>
      </header>
      
      <main className="container mx-auto px-4 py-8">
        <Routes>
          <Route path="/" element={<Navigate to="/teams" replace />} />
          <Route path="/teams" element={<TeamsPage />} />
          <Route 
            path="/teams/:teamId" 
            element={<TeamDetailsPage />} 
          />
          <Route path="/games" element={<GamesPage />} />
          <Route path="/games/create" element={<CreateGamePage />} />
          <Route path="/games/:gameId" element={<GameDetailsPage />} />
        </Routes>
      </main>
    </div>
  )
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
