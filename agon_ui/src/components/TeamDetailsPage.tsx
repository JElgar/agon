import { useEffect } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { ArrowLeft } from 'lucide-react'
import { useGetTeam } from '@/hooks/useApi'
import { AddTeamMemberDialog } from '@/components/AddTeamMemberDialog'

export function TeamDetailsPage() {
  const { teamId } = useParams<{ teamId: string }>()
  const navigate = useNavigate()
  const { data: team, loading, error, getTeam } = useGetTeam()

  useEffect(() => {
    if (teamId) {
      getTeam(teamId)
    }
  }, [teamId, getTeam])

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading team details...</div>
      </div>
    )
  }

  if (error || (!loading && !team)) {
    return (
      <div className="space-y-6">
        <div className="flex items-center space-x-4">
          <Button 
            variant="outline" 
            size="icon"
            onClick={() => navigate('/teams')}
          >
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-2xl font-bold">Team Not Found</h2>
        </div>
        
        <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
          <p className="text-destructive">
            {error || `Team with ID "${teamId}" not found.`}
          </p>
        </div>
        
        <Button onClick={() => navigate('/teams')}>
          Back to Teams
        </Button>
      </div>
    )
  }

  if (!team) {
    return null
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center space-x-4">
        <Button 
          variant="outline" 
          size="icon"
          onClick={() => navigate('/teams')}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <h2 className="text-2xl font-bold">{team.name}</h2>
      </div>

      <div className="grid gap-6">
        {/* Team Info */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Team Information</h3>
          <div className="space-y-2">
            <div>
              <span className="text-sm font-medium text-muted-foreground">Team ID:</span>
              <p className="font-mono text-sm">{team.id}</p>
            </div>
            <div>
              <span className="text-sm font-medium text-muted-foreground">Team Name:</span>
              <p>{team.name}</p>
            </div>
          </div>
        </div>

        {/* Team Members Section */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold">Team Members</h3>
            <AddTeamMemberDialog 
              teamId={team.id} 
              onMemberAdded={() => {
                // Refresh team data when a member is added
                if (teamId) {
                  getTeam(teamId)
                }
              }} 
            />
          </div>
          
          {team.members && team.members.length > 0 ? (
            <div className="space-y-3">
              {team.members.map((member) => (
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
              No team members found. Add your first team member!
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