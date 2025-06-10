import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ArrowLeft, Plus, X } from 'lucide-react'
import { useCreateGame, useSearchUsers, useGetTeams } from '@/hooks/useApi'
import { AddTeamMemberDialog } from '@/components/AddTeamMemberDialog'
import type { GameType, User, CreateGameInput, TeamListItem } from '@/lib/api'

const GAME_TYPES: { value: GameType; label: string }[] = [
  { value: 'football_5_a_side' as GameType, label: '5-a-side Football' },
  { value: 'football_11_a_side' as GameType, label: '11-a-side Football' },
  { value: 'basketball' as GameType, label: 'Basketball' },
  { value: 'tennis' as GameType, label: 'Tennis' },
  { value: 'badminton' as GameType, label: 'Badminton' },
  { value: 'cricket' as GameType, label: 'Cricket' },
  { value: 'rugby' as GameType, label: 'Rugby' },
  { value: 'hockey' as GameType, label: 'Hockey' },
  { value: 'other' as GameType, label: 'Other' },
]

export function CreateGamePage() {
  const navigate = useNavigate()
  const { loading, error, createGame } = useCreateGame()
  const { data: searchResults, searchUsers } = useSearchUsers()
  const { data: teams, getTeams } = useGetTeams()

  // Form state
  const [title, setTitle] = useState('')
  const [gameType, setGameType] = useState<GameType>('football_5_a_side' as GameType)
  const [locationName, setLocationName] = useState('')
  const [latitude, setLatitude] = useState('')
  const [longitude, setLongitude] = useState('')
  const [scheduledDate, setScheduledDate] = useState('')
  const [scheduledTime, setScheduledTime] = useState('')
  const [duration, setDuration] = useState('90')
  
  // Invitation state
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([])
  const [selectedTeams, setSelectedTeams] = useState<TeamListItem[]>([])

  useEffect(() => {
    getTeams()
  }, [getTeams])

  // Search users when query changes
  useEffect(() => {
    if (searchQuery.trim().length >= 2) {
      const timeoutId = setTimeout(() => {
        searchUsers(searchQuery.trim())
      }, 300)
      return () => clearTimeout(timeoutId)
    }
  }, [searchQuery, searchUsers])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!title.trim() || !scheduledDate || !scheduledTime || !latitude || !longitude) {
      return
    }

    // Combine date and time into ISO string
    const scheduledDateTime = new Date(`${scheduledDate}T${scheduledTime}`).toISOString()

    // Collect all invited user IDs
    const invitedUserIds = [
      ...selectedUsers.map(user => user.id),
      // Note: In a real implementation, you'd get team member IDs from selected teams
    ]

    const gameInput: CreateGameInput = {
      title: title.trim(),
      game_type: gameType,
      location: {
        latitude: parseFloat(latitude),
        longitude: parseFloat(longitude),
        name: locationName.trim() || undefined,
      },
      scheduled_time: scheduledDateTime,
      duration_minutes: parseInt(duration),
      invited_user_ids: invitedUserIds,
    }

    const result = await createGame(gameInput)
    if (result) {
      // Navigate to the created game's details page with URL-encoded ID
      navigate(`/games/${encodeURIComponent(result.id)}`)
    }
  }

  const addUser = (user: User) => {
    if (!selectedUsers.some(u => u.id === user.id)) {
      setSelectedUsers(prev => [...prev, user])
    }
    setSearchQuery('')
  }

  const removeUser = (userId: string) => {
    setSelectedUsers(prev => prev.filter(user => user.id !== userId))
  }

  const availableUsers = searchResults?.filter(user => 
    !selectedUsers.some(selected => selected.id === user.id)
  ) || []

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
        <h2 className="text-2xl font-bold">Create New Game</h2>
      </div>

      <form onSubmit={handleSubmit} className="space-y-6">
        {/* Basic Game Info */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Game Details</h3>
          <div className="space-y-4">
            <div>
              <Label htmlFor="title">Game Title</Label>
              <Input
                id="title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="Enter game title"
                required
              />
            </div>

            <div>
              <Label htmlFor="gameType">Game Type</Label>
              <select
                id="gameType"
                value={gameType}
                onChange={(e) => setGameType(e.target.value as GameType)}
                className="w-full px-3 py-2 border border-input rounded-md bg-background"
                required
              >
                {GAME_TYPES.map((type) => (
                  <option key={type.value} value={type.value}>
                    {type.label}
                  </option>
                ))}
              </select>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <Label htmlFor="scheduledDate">Date</Label>
                <Input
                  id="scheduledDate"
                  type="date"
                  value={scheduledDate}
                  onChange={(e) => setScheduledDate(e.target.value)}
                  required
                />
              </div>
              <div>
                <Label htmlFor="scheduledTime">Time</Label>
                <Input
                  id="scheduledTime"
                  type="time"
                  value={scheduledTime}
                  onChange={(e) => setScheduledTime(e.target.value)}
                  required
                />
              </div>
            </div>

            <div>
              <Label htmlFor="duration">Duration (minutes)</Label>
              <Input
                id="duration"
                type="number"
                value={duration}
                onChange={(e) => setDuration(e.target.value)}
                min="30"
                max="300"
                required
              />
            </div>
          </div>
        </div>

        {/* Location */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Location</h3>
          <div className="space-y-4">
            <div>
              <Label htmlFor="locationName">Location Name (Optional)</Label>
              <Input
                id="locationName"
                value={locationName}
                onChange={(e) => setLocationName(e.target.value)}
                placeholder="e.g., Central Park Football Field"
              />
            </div>
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
              <div>
                <Label htmlFor="latitude">Latitude</Label>
                <Input
                  id="latitude"
                  type="number"
                  step="any"
                  value={latitude}
                  onChange={(e) => setLatitude(e.target.value)}
                  placeholder="e.g., 40.7128"
                  required
                />
              </div>
              <div>
                <Label htmlFor="longitude">Longitude</Label>
                <Input
                  id="longitude"
                  type="number"
                  step="any"
                  value={longitude}
                  onChange={(e) => setLongitude(e.target.value)}
                  placeholder="e.g., -74.0060"
                  required
                />
              </div>
            </div>
          </div>
        </div>

        {/* Invitations */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Invite Players</h3>
          
          {/* User Search */}
          <div className="space-y-4">
            <div>
              <Label htmlFor="search">Search Users</Label>
              <Input
                id="search"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Type username or name..."
              />
            </div>

            {/* Search Results */}
            {searchQuery.length >= 2 && availableUsers.length > 0 && (
              <div className="space-y-2">
                <Label>Search Results</Label>
                <div className="max-h-32 overflow-y-auto border rounded-md">
                  {availableUsers.map((user) => (
                    <div
                      key={user.id}
                      className="p-2 hover:bg-muted cursor-pointer border-b last:border-b-0"
                      onClick={() => addUser(user)}
                    >
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="font-medium">{user.first_name} {user.last_name}</p>
                          <p className="text-sm text-muted-foreground">@{user.username}</p>
                        </div>
                        <Plus className="h-4 w-4 text-muted-foreground" />
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Selected Users */}
            {selectedUsers.length > 0 && (
              <div className="space-y-2">
                <Label>Invited Players ({selectedUsers.length})</Label>
                <div className="space-y-2">
                  {selectedUsers.map((user) => (
                    <div
                      key={user.id}
                      className="flex items-center justify-between p-2 border rounded-md bg-muted/50"
                    >
                      <div>
                        <p className="font-medium">{user.first_name} {user.last_name}</p>
                        <p className="text-sm text-muted-foreground">@{user.username}</p>
                      </div>
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => removeUser(user.id)}
                      >
                        <X className="h-4 w-4" />
                      </Button>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        </div>

        {error && (
          <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
            <p className="text-destructive text-sm">{error}</p>
          </div>
        )}

        <div className="flex justify-end space-x-4">
          <Button 
            type="button" 
            variant="outline" 
            onClick={() => navigate('/games')}
            disabled={loading}
          >
            Cancel
          </Button>
          <Button type="submit" disabled={loading}>
            {loading ? 'Creating...' : 'Create Game'}
          </Button>
        </div>
      </form>
    </div>
  )
}