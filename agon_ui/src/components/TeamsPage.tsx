import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { useGetTeams, useCreateTeam } from '@/hooks/useApi'
import { debugJwt } from '@/utils/jwt-debug'

export function TeamsPage() {
  const navigate = useNavigate()
  const { data: teams, loading: teamsLoading, error: teamsError, getTeams } = useGetTeams()
  const { loading: createLoading, error: createError, createTeam } = useCreateTeam()

  useEffect(() => {
    debugJwt() // Debug JWT token
    getTeams()
  }, [getTeams])

  const handleCreateTeam = async () => {
    const result = await createTeam({ name: `Team ${Date.now()}` })
    if (result) {
      // Refresh teams list
      getTeams()
    }
  }

  if (teamsLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading teams...</div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-bold">Teams</h2>
        <Button 
          onClick={handleCreateTeam}
          disabled={createLoading}
        >
          {createLoading ? 'Creating...' : 'Create Team'}
        </Button>
      </div>

      {teamsError && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-md">
          <p className="text-red-800">Error loading teams: {teamsError}</p>
        </div>
      )}

      {createError && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-md">
          <p className="text-red-800">Error creating team: {createError}</p>
        </div>
      )}

      <div className="grid gap-4">
        {teams && teams.length > 0 ? (
          teams.map((team) => (
            <div 
              key={team.id}
              className="p-4 border border-border rounded-lg hover:bg-muted/50 cursor-pointer transition-colors"
              onClick={() => navigate(`/teams/${team.id}`)}
            >
              <h3 className="font-semibold">{team.name}</h3>
              <p className="text-sm text-muted-foreground">ID: {team.id}</p>
              <p className="text-xs text-muted-foreground mt-2">Click to view details</p>
            </div>
          ))
        ) : (
          <div className="text-center py-8 text-muted-foreground">
            No teams found. Create your first team!
          </div>
        )}
      </div>
    </div>
  )
}