import { useEffect } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { ArrowLeft, Calendar, MapPin, Clock } from 'lucide-react'
import { useGetGroup, useGetGroupGames } from '@/hooks/useApi'
import { AddGroupMemberDialog } from '@/components/AddGroupMemberDialog'

export function GroupDetailsPage() {
  const { groupId } = useParams<{ groupId: string }>()
  const navigate = useNavigate()
  const { data: group, loading, error, getGroup } = useGetGroup()
  const { data: groupGames, loading: loadingGames, error: gamesError } = useGetGroupGames(groupId)

  useEffect(() => {
    if (groupId) {
      getGroup(groupId)
    }
  }, [groupId, getGroup])

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading group details...</div>
      </div>
    )
  }

  if (error || (!loading && !group)) {
    return (
      <div className="space-y-6">
        <div className="flex items-center space-x-4">
          <Button 
            variant="outline" 
            size="icon"
            onClick={() => navigate('/groups')}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-2xl font-bold">Group Not Found</h2>
        </div>
        
        <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
          <p className="text-destructive">
            {error || `Group with ID "${groupId}" not found.`}
          </p>
        </div>
        
        <Button onClick={() => navigate('/groups')}>
          Back to Groups
        </Button>
      </div>
    )
  }

  if (!group) {
    return null
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center space-x-4">
        <Button 
          variant="outline" 
          size="icon"
          onClick={() => navigate('/groups')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h2 className="text-2xl font-bold">{group.name}</h2>
      </div>

      <div className="grid gap-6">
        {/* Team Info */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Team Information</h3>
          <div className="space-y-2">
            <div>
              <span className="text-sm font-medium text-muted-foreground">Team ID:</span>
              <p className="font-mono text-sm">{group.id}</p>
            </div>
            <div>
              <span className="text-sm font-medium text-muted-foreground">Team Name:</span>
              <p>{group.name}</p>
            </div>
          </div>
        </div>

        {/* Team Members Section */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold">Team Members</h3>
            <AddGroupMemberDialog
              groupId={group?.id}
              onMemberAdded={() => {
                // Refresh group data when a member is added
                if (groupId) {
                  getGroup(groupId)
                }
              }} 
            />
          </div>
          
          {group.members && group.members.length > 0 ? (
            <div className="space-y-3">
              {group.members.map((member) => (
                <div 
                  key={member.id} 
                  className="flex items-center justify-between p-3 border border-border rounded-lg"
                >
                  <div>
                    <p className="font-medium">{member.first_name} {member.last_name}</p>
                    <p className="text-sm text-muted-foreground">{member.email}</p>
                  </div>
                  <div className="text-xs text-muted-foreground font-mono">
                    ID: {member.id}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-center py-8 text-muted-foreground">
              No group members found. Add your first group member!
            </div>
          )}
        </div>

        {/* Games Section */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Group Games</h3>
          
          {loadingGames ? (
            <div className="text-center py-8 text-muted-foreground">
              Loading games...
            </div>
          ) : gamesError ? (
            <div className="text-center py-8 text-destructive">
              Failed to load group games: {gamesError}
            </div>
          ) : groupGames && groupGames.length > 0 ? (
            <div className="space-y-3">
              {groupGames.map((game) => (
                <div 
                  key={game.id} 
                  className="flex items-center justify-between p-3 border border-border rounded-lg hover:bg-muted/50 cursor-pointer"
                  onClick={() => navigate(`/games/${encodeURIComponent(game.id)}`)}
                >
                  <div className="flex-1">
                    <div className="flex items-center space-x-2 mb-1">
                      <h4 className="font-medium">{game.title}</h4>
                      <span className="text-xs bg-muted px-2 py-1 rounded">{game.game_type.replace('_', ' ')}</span>
                    </div>
                    <div className="flex items-center space-x-4 text-sm text-muted-foreground">
                      <div className="flex items-center space-x-1">
                        <Calendar className="h-3 w-3" />
                        <span>{new Date(game.scheduled_time).toLocaleDateString()}</span>
                      </div>
                      <div className="flex items-center space-x-1">
                        <Clock className="h-3 w-3" />
                        <span>{new Date(game.scheduled_time).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
                      </div>
                      <div className="flex items-center space-x-1">
                        <MapPin className="h-3 w-3" />
                        <span>{game.location.name || `${game.location.latitude}, ${game.location.longitude}`}</span>
                      </div>
                    </div>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {game.duration_minutes}min
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-center py-8 text-muted-foreground">
              No games found where this group was invited.
            </div>
          )}
        </div>

        {/* Team Settings */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Team Settings</h3>
          <div className="space-y-4">
            <Button variant="outline">
              Edit Team Name
            </Button>
            <Button variant="destructive">
              Delete Team
            </Button>
          </div>
        </div>
      </div>
    </div>
  )
}
