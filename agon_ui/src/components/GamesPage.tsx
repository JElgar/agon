import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { useGetGames } from '@/hooks/useApi'
import { Plus, Calendar, MapPin, Users, Clock, RotateCcw } from 'lucide-react'
import type { Game } from '@/lib/api'

export function GamesPage() {
  const navigate = useNavigate()
  const { data: games, loading, error } = useGetGames()

  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      weekday: 'short',
      month: 'short',
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

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'scheduled': return 'bg-blue-100 text-blue-800'
      case 'in_progress': return 'bg-green-100 text-green-800'
      case 'completed': return 'bg-gray-100 text-gray-800'
      case 'cancelled': return 'bg-red-100 text-red-800'
      default: return 'bg-gray-100 text-gray-800'
    }
  }

  const formatScheduleInfo = (game: Game) => {
    if (game.schedule.type === 'recurring') {
      return `Recurring game (${game.schedule.cron_schedule})`
    }
    return 'One-time game'
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading games...</div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-bold">Games</h2>
        <Button onClick={() => navigate('/games/create')}>
          <Plus className="h-4 w-4 mr-2" />
          Create Game
        </Button>
      </div>

      {error && (
        <div className="p-4 bg-red-50 border border-red-200 rounded-md">
          <p className="text-red-800">Error loading games: {error}</p>
        </div>
      )}

      <div className="grid gap-4">
        {games && games.length > 0 ? (
          games.map((game) => (
            <div 
              key={game.id}
              className="p-6 border border-border rounded-lg hover:bg-muted/50 cursor-pointer transition-colors"
              onClick={() => navigate(`/games/${encodeURIComponent(game.id)}`)}
            >
              <div className="flex items-start justify-between mb-4">
                <div className="flex-1">
                  <h3 className="text-lg font-semibold mb-1 flex items-center">
                    {game.title}
                    {game.schedule.type === 'recurring' && <RotateCcw className="h-4 w-4 ml-2 text-muted-foreground" />}
                  </h3>
                  <div className="flex items-center text-sm text-muted-foreground mb-2">
                    <span className="font-medium">{formatGameType(game.game_type)}</span>
                    <span className={`ml-2 px-2 py-1 rounded-full text-xs font-medium ${getStatusColor(game.status)}`}>
                      {game.status.charAt(0).toUpperCase() + game.status.slice(1)}
                    </span>
                  </div>
                  <div className="text-xs text-muted-foreground">
                    {formatScheduleInfo(game)}
                  </div>
                </div>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-sm">
                <div className="flex items-center text-muted-foreground">
                  <Calendar className="h-4 w-4 mr-2" />
                  {formatDate(game.scheduled_time)}
                </div>
                
                <div className="flex items-center text-muted-foreground">
                  <Clock className="h-4 w-4 mr-2" />
                  {game.duration_minutes} minutes
                </div>
                
                <div className="flex items-center text-muted-foreground">
                  <MapPin className="h-4 w-4 mr-2" />
                  {game.location.name || `${game.location.latitude}, ${game.location.longitude}`}
                </div>
              </div>

              <p className="text-xs text-muted-foreground mt-4">
                Click to view details and manage invitations
              </p>
            </div>
          ))
        ) : (
          <div className="text-center py-12">
            <Users className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
            <h3 className="text-lg font-medium mb-2">No games yet</h3>
            <p className="text-muted-foreground mb-4">
              Create your first game to get started!
            </p>
            <Button onClick={() => navigate('/games/create')}>
              <Plus className="h-4 w-4 mr-2" />
              Create Game
            </Button>
          </div>
        )}
      </div>
    </div>
  )
}
