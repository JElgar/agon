import { useEffect, useState } from 'react'
import { useParams, useNavigate, useSearchParams } from 'react-router-dom'
import { useAuth } from '@/hooks/useAuth'
import { 
  useGetGroupByInviteToken, 
  useJoinGroupViaInvite,
  useGetTeamByInviteToken,
  useJoinTeamViaInvite 
} from '@/hooks/useApi'
import { Button } from '@/components/ui/button'

export function InviteLinkHandler() {
  const { token } = useParams<{ token: string }>()
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const { session, loading: authLoading } = useAuth()
  const isAuthenticated = session !== null
  const [inviteType, setInviteType] = useState<'group' | 'team' | null>(null)
  const [joinAttempted, setJoinAttempted] = useState(false)


  // Try to determine invite type from URL or search params
  useEffect(() => {
    const type = searchParams.get('type')
    if (type === 'group' || type === 'team') {
      setInviteType(type)
    } else {
      // Default to trying group first, then team if that fails
      setInviteType('group')
    }
  }, [searchParams])

  // Group invite handling
  const { 
    data: groupData, 
    loading: groupLoading, 
    error: groupError 
  } = useGetGroupByInviteToken(inviteType === 'group' ? token : undefined)

  const { 
    loading: joiningGroup, 
    error: joinGroupError, 
    joinGroup 
  } = useJoinGroupViaInvite()

  // Team invite handling
  const { 
    data: teamData, 
    loading: teamLoading, 
    error: teamError 
  } = useGetTeamByInviteToken(inviteType === 'team' ? token : undefined)

  const { 
    loading: joiningTeam, 
    error: joinTeamError, 
    joinTeam 
  } = useJoinTeamViaInvite()

  // Handle login redirect with fromLogin parameter
  const handleLogin = () => {
    const returnUrl = `/invite/${token}${inviteType ? `?type=${inviteType}&fromLogin=true` : '?fromLogin=true'}`
    navigate(`/login?returnUrl=${encodeURIComponent(returnUrl)}`)
  }

  // Handle manual join for authenticated users
  const handleJoin = async () => {
    if (!isAuthenticated) {
      handleLogin()
      return
    }

    setJoinAttempted(true)
    try {
      if (groupData && token) {
        await joinGroup(token)
        navigate(`/groups/${encodeURIComponent(groupData.id)}`)
      } else if (teamData && token) {
        await joinTeam(token)
        navigate(`/games/${encodeURIComponent(teamData.game.id)}`)
      }
    } catch (error) {
      console.error('Failed to join:', error)
      setJoinAttempted(false)
    }
  }

  // Try team invite if group invite fails
  useEffect(() => {
    if (inviteType === 'group' && groupError && !teamData && !teamError) {
      setInviteType('team')
    }
  }, [inviteType, groupError, teamData, teamError])

  // Auto-join logic after authentication
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search)
    const fromLogin = urlParams.get('fromLogin') === 'true'
    
    // Auto-join if user came from login redirect
    if (isAuthenticated && !joinAttempted && (groupData || teamData) && fromLogin) {
      setJoinAttempted(true)
      
      if (groupData && token) {
        joinGroup(token)
          .then(() => {
            navigate(`/groups/${encodeURIComponent(groupData.id)}`)
          })
          .catch((error) => {
            console.error('Failed to join group:', error)
            setJoinAttempted(false)
          })
      } else if (teamData && token) {
        joinTeam(token)
          .then(() => {
            navigate(`/games/${encodeURIComponent(teamData.game.id)}`)
          })
          .catch((error) => {
            console.error('Failed to join team:', error)
            setJoinAttempted(false)
          })
      }
    }
  }, [isAuthenticated, joinAttempted, groupData, teamData, token, joinGroup, joinTeam, navigate])

  // Loading states
  if (authLoading || (groupLoading && teamLoading)) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-gray-900 mx-auto mb-4"></div>
          <p>Loading...</p>
        </div>
      </div>
    )
  }

  // Show loading for unauthenticated users while fetching invite data
  if (!isAuthenticated && (groupLoading || teamLoading) && !groupData && !teamData) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-gray-900 mx-auto mb-4"></div>
          <p>Loading invite...</p>
        </div>
      </div>
    )
  }

  // Show invite preview for unauthenticated users
  if (!isAuthenticated && (groupData || teamData)) {
    const isGroup = !!groupData

    return (
      <div className="flex items-center justify-center min-h-screen bg-background">
        <div className="text-center max-w-md p-6 border border-border rounded-lg bg-card shadow-lg">
          <h1 className="text-2xl font-bold mb-4">
            You're Invited!
          </h1>
          <div className="bg-muted p-4 rounded-lg mb-6">
            <h2 className="text-xl font-semibold mb-2">
              {isGroup ? groupData?.name : teamData?.team.name}
            </h2>
            <p className="text-muted-foreground text-sm mb-2">
              {isGroup ? 'Group' : 'Team'}
            </p>
            {!isGroup && teamData && (
              <div className="text-sm text-muted-foreground">
                <p><strong>Game:</strong> {teamData.game.title}</p>
                <p><strong>Type:</strong> {teamData.game.game_type.replace('_', ' ')}</p>
                <p><strong>Date:</strong> {new Date(teamData.game.scheduled_time).toLocaleDateString()}</p>
              </div>
            )}
          </div>
          <p className="text-sm text-muted-foreground mb-6">
            Sign in to join this {isGroup ? 'group' : 'team'}
          </p>
          <Button onClick={handleLogin} className="w-full">
            Sign In to Join
          </Button>
        </div>
      </div>
    )
  }

  // Additional loading states for invite data or joining
  if ((groupLoading || teamLoading) && !groupData && !teamData) {
    return (
      <div className="flex items-center justify-center min-h-screen">
        <div className="text-center">
          <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-gray-900 mx-auto mb-4"></div>
          <p>Loading invite...</p>
        </div>
      </div>
    )
  }

  // Error states for both authenticated and unauthenticated users
  const currentError = groupError || teamError
  if (currentError && !groupData && !teamData) {
    return (
      <div className="flex items-center justify-center min-h-screen bg-background">
        <div className="text-center max-w-md p-6 border border-border rounded-lg bg-card shadow-lg">
          <h1 className="text-2xl font-bold text-destructive mb-4">Invalid Invite</h1>
          <p className="text-muted-foreground mb-6">
            This invite link is invalid or has expired.
          </p>
          {isAuthenticated ? (
            <Button onClick={() => navigate('/groups')}>
              Go to Groups
            </Button>
          ) : (
            <div className="space-y-3">
              <Button onClick={handleLogin} variant="outline" className="w-full">
                Sign In
              </Button>
              <p className="text-xs text-muted-foreground">
                Sign in to see if you have access to other invites
              </p>
            </div>
          )}
        </div>
      </div>
    )
  }

  // Show invite preview for authenticated users (manual join)
  const inviteData = groupData || teamData
  const isGroup = !!groupData

  if (inviteData && isAuthenticated) {
    return (
      <div className="flex items-center justify-center min-h-screen bg-background">
        <div className="text-center max-w-md p-6 border border-border rounded-lg bg-card shadow-lg">
          <h1 className="text-2xl font-bold mb-4">
            Join {isGroup ? 'Group' : 'Team'}
          </h1>
          <div className="bg-muted p-4 rounded-lg mb-6">
            <h2 className="text-xl font-semibold mb-2">
              {isGroup ? groupData?.name : teamData?.team.name}
            </h2>
            <p className="text-muted-foreground text-sm mb-2">
              {isGroup ? 'Group' : 'Team'}
            </p>
            {!isGroup && teamData && (
              <div className="text-sm text-muted-foreground">
                <p><strong>Game:</strong> {teamData.game.title}</p>
                <p><strong>Type:</strong> {teamData.game.game_type.replace('_', ' ')}</p>
                <p><strong>Date:</strong> {new Date(teamData.game.scheduled_time).toLocaleDateString()}</p>
              </div>
            )}
          </div>
          
          {joinGroupError || joinTeamError ? (
            <div className="mb-4 p-3 bg-destructive/10 border border-destructive/20 rounded-md">
              <p className="text-sm text-destructive">{joinGroupError || joinTeamError}</p>
            </div>
          ) : null}
          
          <Button 
            onClick={handleJoin}
            disabled={joiningGroup || joiningTeam}
            className="w-full"
          >
            {joiningGroup || joiningTeam ? 'Joining...' : `Join ${isGroup ? 'Group' : 'Team'}`}
          </Button>
        </div>
      </div>
    )
  }

  // Fallback for unauthenticated users with no data
  if (!isAuthenticated) {
    return (
      <div className="flex items-center justify-center min-h-screen bg-background">
        <div className="text-center max-w-md p-6 border border-border rounded-lg bg-card shadow-lg">
          <h1 className="text-2xl font-bold mb-4">Invite Link</h1>
          <p className="text-muted-foreground mb-6">
            Unable to load invite information. Please try again or sign in to continue.
          </p>
          <Button onClick={handleLogin} className="w-full">
            Sign In
          </Button>
        </div>
      </div>
    )
  }

  // Fallback for authenticated users
  return (
    <div className="flex items-center justify-center min-h-screen">
      <div className="text-center">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-gray-900 mx-auto mb-4"></div>
        <p>Processing invite...</p>
      </div>
    </div>
  )
}