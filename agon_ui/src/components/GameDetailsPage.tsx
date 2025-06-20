import { useState, useEffect } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { ArrowLeft, Calendar, MapPin, Clock, Users, CheckCircle, XCircle, RotateCcw } from 'lucide-react'
import { useGetGameDetails, useRespondToInvitation, useGetGroup } from '@/hooks/useApi'
import { useAuth } from '@/hooks/useAuth'
import { InviteMorePeopleDialog } from '@/components/InviteMorePeopleDialog'
import type { components } from '@/types/api'
import type { Game } from '@/lib/api'

type InvitationResponse = components['schemas']['InvitationResponse']
type InvitationStatus = components['schemas']['InvitationStatus']

export function GameDetailsPage() {
  const { gameId: encodedGameId } = useParams<{ gameId: string }>()
  const navigate = useNavigate()
  const { user } = useAuth()
  
  // Decode the game ID from URL
  const gameId = encodedGameId ? decodeURIComponent(encodedGameId) : undefined
  
  const { data: gameData, loading, error, refetch } = useGetGameDetails(gameId)
  const { loading: respondLoading, respondToInvitation } = useRespondToInvitation()
  const { getGroup } = useGetGroup()
  const [isUpdating, setIsUpdating] = useState(false)
  const [showInviteDialog, setShowInviteDialog] = useState(false)
  const [groupNames, setGroupNames] = useState<Record<string, string>>({})

  // Fetch group names for invitations that came from groups
  useEffect(() => {
    const fetchGroupNames = async () => {
      if (!gameData?.invitations) return

      const groupIds = new Set<string>()
      gameData.invitations.forEach(inv => {
        if (inv.invitation.group_id) {
          groupIds.add(inv.invitation.group_id)
        }
      })

      const newGroupNames: Record<string, string> = {}
      for (const groupId of groupIds) {
        try {
          const group = await getGroup(groupId)
          if (group) {
            newGroupNames[groupId] = group.name
          }
        } catch (error) {
          console.error(`Failed to fetch group ${groupId}:`, error)
          newGroupNames[groupId] = 'Unknown Group'
        }
      }

      setGroupNames(newGroupNames)
    }

    fetchGroupNames()
  }, [gameData?.invitations, getGroup])

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      weekday: 'long',
      year: 'numeric',
      month: 'long',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  const formatGameType = (gameType: string) => {
    return gameType.split('_').map(word => 
      word.charAt(0).toUpperCase() + word.slice(1)
    ).join(' ')
  }

  const formatScheduleInfo = (game: Game) => {
    if (game.schedule.type === 'recurring') {
      return {
        type: 'Recurring Game',
        details: `Every ${game.schedule.cron_schedule}`,
        occurrence: `This occurrence: ${new Date(game.schedule.occurrence_date).toLocaleDateString()}`,
        range: game.schedule.end_date 
          ? `Until ${new Date(game.schedule.end_date).toLocaleDateString()}`
          : 'No end date'
      }
    }
    return {
      type: 'One-time Game',
      details: 'Single occurrence',
      occurrence: null,
      range: null
    }
  }

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'scheduled': return 'bg-blue-100 text-blue-800'
      case 'in_progress': return 'bg-green-100 text-green-800'
      case 'completed': return 'bg-gray-100 text-gray-800'
      case 'cancelled': return 'bg-red-100 text-red-800'
      default: return 'bg-gray-100 text-gray-800'
    }
  }

  const getInvitationStatusColor = (status: InvitationStatus) => {
    switch (status) {
      case 'pending': return 'bg-yellow-100 text-yellow-800'
      case 'accepted': return 'bg-green-100 text-green-800'
      case 'declined': return 'bg-red-100 text-red-800'
      default: return 'bg-gray-100 text-gray-800'
    }
  }

  const handleInvitationResponse = async (response: InvitationResponse) => {
    if (!gameId || !user?.id) return
    
    setIsUpdating(true)
    const currentStatus = currentUserInvitation?.invitation.status
    const isChanging = currentStatus && currentStatus !== 'pending'
    
    console.log(`${isChanging ? 'Updating' : 'Responding to'} invitation:`, { 
      gameId, 
      response, 
      previousStatus: currentStatus 
    })
    
    try {
      const result = await respondToInvitation(gameId, response)
      console.log('Invitation response result:', result)
      
      if (result === null) {
        console.error('Failed to update invitation response')
      }
      // React Query will automatically refetch the game details due to cache invalidation
    } catch (error) {
      console.error('Error updating invitation response:', error)
    } finally {
      setIsUpdating(false)
    }
  }

  const currentUserInvitation = gameData?.invitations.find(
    inv => inv.user.id === user?.id
  )

  const acceptedInvitations = gameData?.invitations.filter(
    inv => inv.invitation.status === 'accepted'
  ) || []

  const pendingInvitations = gameData?.invitations.filter(
    inv => inv.invitation.status === 'pending'
  ) || []

  const declinedInvitations = gameData?.invitations.filter(
    inv => inv.invitation.status === 'declined'
  ) || []

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading game details...</div>
      </div>
    )
  }

  if (error || !gameData) {
    return (
      <div className="space-y-6">
        <div className="flex items-center space-x-4">
          <Button 
            variant="outline" 
            size="icon"
            onClick={() => navigate('/games')}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-2xl font-bold">Game Not Found</h2>
        </div>
        <div className="p-4 bg-red-50 border border-red-200 rounded-md">
          <p className="text-red-800">{error || 'Game not found'}</p>
        </div>
      </div>
    )
  }

  const game = gameData.game

  return (
    <div className="space-y-6">
      <div className="flex items-center space-x-4">
        <Button 
          variant="outline" 
          size="icon"
          onClick={() => navigate('/games')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h2 className="text-2xl font-bold">{game.title}</h2>
      </div>

      {/* Game Details Card */}
      <div className="p-6 border border-border rounded-lg bg-card">
        <div className="flex items-start justify-between mb-4">
          <div className="flex-1">
            <div className="flex items-center text-sm mb-2">
              <span className="font-medium">{formatGameType(game.game_type)}</span>
              <span className={`ml-2 px-2 py-1 rounded-full text-xs font-medium ${getStatusColor(game.status)}`}>
                {game.status.charAt(0).toUpperCase() + game.status.slice(1)}
              </span>
            </div>
          </div>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
          <div className="flex items-center">
            <Calendar className="h-4 w-4 mr-2" />
            <div>
              <p className="font-medium">Scheduled</p>
              <p className="text-sm">{formatDate(game.scheduled_time)}</p>
            </div>
          </div>
          
          <div className="flex items-center">
            <Clock className="h-4 w-4 mr-2" />
            <div>
              <p className="font-medium">Duration</p>
              <p className="text-sm">{game.duration_minutes} minutes</p>
            </div>
          </div>
          
          <div className="flex items-center">
            <MapPin className="h-4 w-4 mr-2" />
            <div>
              <p className="font-medium">Location</p>
              <p className="text-sm">{game.location.name || `${game.location.latitude}, ${game.location.longitude}`}</p>
            </div>
          </div>

          <div className="flex items-center">
            {game.schedule.type === 'recurring' ? <RotateCcw className="h-4 w-4 mr-2" /> : <Calendar className="h-4 w-4 mr-2" />}
            <div>
              <p className="font-medium">{formatScheduleInfo(game).type}</p>
              <p className="text-sm">{formatScheduleInfo(game).details}</p>
              {formatScheduleInfo(game).occurrence && (
                <p className="text-xs text-muted-foreground">{formatScheduleInfo(game).occurrence}</p>
              )}
            </div>
          </div>
        </div>

        {/* Current User's Invitation Response */}
        {currentUserInvitation && (
          <div className="mb-6 p-4 border rounded-md bg-muted/50">
            <h4 className="font-medium mb-2">Your Invitation</h4>
            {currentUserInvitation.invitation.group_id && groupNames[currentUserInvitation.invitation.group_id] && (
              <p className="text-sm text-muted-foreground mb-2">
                Invited via group: <span className="font-medium">{groupNames[currentUserInvitation.invitation.group_id]}</span>
              </p>
            )}
            <div className="flex items-center justify-between">
              <div className="flex items-center space-x-2">
                <span className={`px-2 py-1 rounded-full text-xs font-medium ${getInvitationStatusColor(currentUserInvitation.invitation.status)}`}>
                  {currentUserInvitation.invitation.status.charAt(0).toUpperCase() + currentUserInvitation.invitation.status.slice(1)}
                </span>
                {currentUserInvitation.invitation.responded_at && (
                  <span className="text-xs text-muted-foreground">
                    Responded: {new Date(currentUserInvitation.invitation.responded_at).toLocaleDateString()}
                  </span>
                )}
              </div>
              
              <div className="flex space-x-2">
                {currentUserInvitation.invitation.status !== 'accepted' && (
                  <Button
                    size="sm"
                    onClick={() => handleInvitationResponse('accepted')}
                    disabled={respondLoading || isUpdating}
                    variant="outline"
                  >
                    <CheckCircle className="h-4 w-4 mr-1" />
                    {isUpdating ? 'Updating...' : 
                     currentUserInvitation.invitation.status === 'pending' ? 'Accept' : 'Change to Accept'}
                  </Button>
                )}
                {currentUserInvitation.invitation.status !== 'declined' && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => handleInvitationResponse('declined')}
                    disabled={respondLoading || isUpdating}
                  >
                    <XCircle className="h-4 w-4 mr-1" />
                    {isUpdating ? 'Updating...' : 
                     currentUserInvitation.invitation.status === 'pending' ? 'Decline' : 'Change to Decline'}
                  </Button>
                )}
              </div>
            </div>
            
            {/* Status-specific messaging */}
            <div className="mt-2 text-sm text-muted-foreground">
              {currentUserInvitation.invitation.status === 'pending' && (
                <p>Please respond to this invitation.</p>
              )}
              {currentUserInvitation.invitation.status === 'accepted' && (
                <p>You're attending this game! You can change your mind by declining.</p>
              )}
              {currentUserInvitation.invitation.status === 'declined' && (
                <p>You've declined this invitation. You can change your mind by accepting.</p>
              )}
            </div>
          </div>
        )}

        {/* Invite More People Button */}
        <div className="flex justify-end">
          <Button
            onClick={() => setShowInviteDialog(true)}
            variant="outline"
          >
            <Users className="h-4 w-4 mr-2" />
            Invite More People
          </Button>
        </div>
      </div>

      {/* Teams and Players */}
      {gameData.teams.length > 1 ? (
        /* Teams Mode - Show individual teams */
        <div className="space-y-6">
          <h3 className="text-lg font-semibold">Teams</h3>
          <div className="grid gap-6 md:grid-cols-2">
            {gameData.teams.map((team) => (
              <div key={team.id} className="p-6 border border-border rounded-lg bg-card">
                <h4 className="text-lg font-semibold mb-4 flex items-center">
                  {team.color && (
                    <div 
                      className="w-4 h-4 rounded mr-2 border"
                      style={{ backgroundColor: team.color }}
                    ></div>
                  )}
                  {team.name} ({team.members.length})
                </h4>
                <div className="space-y-2">
                  {team.members.map((member) => {
                    const invitation = gameData.invitations.find(inv => inv.user.id === member.id)
                    const status = invitation?.invitation.status || 'unknown'
                    const groupId = invitation?.invitation.group_id
                    const groupName = groupId ? groupNames[groupId] : null
                    
                    return (
                      <div key={member.id} className="flex items-center justify-between p-2 border rounded-md">
                        <div className="flex-1">
                          <p className="font-medium">{member.first_name} {member.last_name}</p>
                          <p className="text-sm text-muted-foreground">@{member.username}</p>
                          {groupName && (
                            <p className="text-xs text-muted-foreground">
                              via group: <span className="font-medium">{groupName}</span>
                            </p>
                          )}
                        </div>
                        <span className={`px-2 py-1 rounded-full text-xs font-medium ${status === 'unknown' ? 'bg-gray-100 text-gray-800' : getInvitationStatusColor(status as InvitationStatus)}`}>
                          {status.charAt(0).toUpperCase() + status.slice(1)}
                        </span>
                      </div>
                    )
                  })}
                  {team.members.length === 0 && (
                    <p className="text-sm text-muted-foreground text-center py-4">No players assigned to this team</p>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      ) : (
        /* Casual Mode - Show players by status */
        <div className="grid gap-6 md:grid-cols-3">
          {/* Accepted */}
          <div className="p-6 border border-border rounded-lg bg-card">
            <h3 className="text-lg font-semibold mb-4 flex items-center">
              <Users className="h-5 w-5 mr-2 text-green-600" />
              Accepted ({acceptedInvitations.length})
            </h3>
            <div className="space-y-2">
              {acceptedInvitations.map((invitation) => {
                const groupId = invitation.invitation.group_id
                const groupName = groupId ? groupNames[groupId] : null
                
                return (
                  <div key={invitation.user.id} className="flex items-center justify-between p-2 border rounded-md">
                    <div className="flex-1">
                      <p className="font-medium">{invitation.user.first_name} {invitation.user.last_name}</p>
                      <p className="text-sm text-muted-foreground">@{invitation.user.username}</p>
                      {groupName && (
                        <p className="text-xs text-muted-foreground">
                          via group: <span className="font-medium">{groupName}</span>
                        </p>
                      )}
                    </div>
                    <CheckCircle className="h-4 w-4 text-green-600" />
                  </div>
                )
              })}
              {acceptedInvitations.length === 0 && (
                <p className="text-sm text-muted-foreground">No accepted invitations yet</p>
              )}
            </div>
          </div>

          {/* Pending */}
          <div className="p-6 border border-border rounded-lg bg-card">
            <h3 className="text-lg font-semibold mb-4 flex items-center">
              <Clock className="h-5 w-5 mr-2 text-yellow-600" />
              Pending ({pendingInvitations.length})
            </h3>
            <div className="space-y-2">
              {pendingInvitations.map((invitation) => {
                const groupId = invitation.invitation.group_id
                const groupName = groupId ? groupNames[groupId] : null
                
                return (
                  <div key={invitation.user.id} className="flex items-center justify-between p-2 border rounded-md">
                    <div className="flex-1">
                      <p className="font-medium">{invitation.user.first_name} {invitation.user.last_name}</p>
                      <p className="text-sm text-muted-foreground">@{invitation.user.username}</p>
                      {groupName && (
                        <p className="text-xs text-muted-foreground">
                          via group: <span className="font-medium">{groupName}</span>
                        </p>
                      )}
                    </div>
                    <Clock className="h-4 w-4 text-yellow-600" />
                  </div>
                )
              })}
              {pendingInvitations.length === 0 && (
                <p className="text-sm text-muted-foreground">No pending invitations</p>
              )}
            </div>
          </div>

          {/* Declined */}
          <div className="p-6 border border-border rounded-lg bg-card">
            <h3 className="text-lg font-semibold mb-4 flex items-center">
              <XCircle className="h-5 w-5 mr-2 text-red-600" />
              Declined ({declinedInvitations.length})
            </h3>
            <div className="space-y-2">
              {declinedInvitations.map((invitation) => {
                const groupId = invitation.invitation.group_id
                const groupName = groupId ? groupNames[groupId] : null
                
                return (
                  <div key={invitation.user.id} className="flex items-center justify-between p-2 border rounded-md">
                    <div className="flex-1">
                      <p className="font-medium">{invitation.user.first_name} {invitation.user.last_name}</p>
                      <p className="text-sm text-muted-foreground">@{invitation.user.username}</p>
                      {groupName && (
                        <p className="text-xs text-muted-foreground">
                          via group: <span className="font-medium">{groupName}</span>
                        </p>
                      )}
                    </div>
                    <XCircle className="h-4 w-4 text-red-600" />
                  </div>
                )
              })}
              {declinedInvitations.length === 0 && (
                <p className="text-sm text-muted-foreground">No declined invitations</p>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Invite More People Dialog */}
      {gameData && (
        <InviteMorePeopleDialog
          gameId={gameId!}
          isOpen={showInviteDialog}
          onClose={() => setShowInviteDialog(false)}
          onInvitesSent={() => {
            // Refresh game details to show new invitations
            refetch()
          }}
          existingInvitations={gameData.invitations.map(inv => inv.user)}
          teams={gameData.teams}
        />
      )}
    </div>
  )
}
