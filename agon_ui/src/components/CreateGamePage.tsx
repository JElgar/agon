import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ArrowLeft, Plus, X, GripVertical } from 'lucide-react'
import { useCreateGame, useSearchUsers } from '@/hooks/useApi'
import {
  DndContext,
  DragOverlay,
  closestCenter,
  useSensor,
  useSensors,
  PointerSensor,
  useDroppable,
} from '@dnd-kit/core'
import type { DragEndEvent, DragStartEvent } from '@dnd-kit/core'
import { SortableContext, verticalListSortingStrategy, useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import type { components } from '@/types/api'

type GameType = components['schemas']['GameType']
type User = components['schemas']['User']  
type CreateGameInput = components['schemas']['CreateGameInput']
type CreateGameTeamInput = components['schemas']['CreateGameTeamInput']

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

// Draggable User Component
function DraggableUser({ 
  user, 
  onRemove,
  isDragOverlay = false
}: { 
  user: User
  onRemove: () => void
  isDragOverlay?: boolean
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: user.id })

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  }

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`flex items-center justify-between p-2 border border-border rounded-md bg-card ${
        isDragOverlay ? 'shadow-lg' : ''
      } ${isDragging ? 'ring-2 ring-blue-500 dark:ring-blue-400' : ''}`}
    >
      <div className="flex items-center space-x-2">
        <div
          {...attributes}
          {...listeners}
          className="cursor-grab active:cursor-grabbing p-1 hover:bg-muted/50 rounded"
        >
          <GripVertical className="h-4 w-4 text-muted-foreground" />
        </div>
        <div>
          <p className="font-medium text-sm">{user.first_name} {user.last_name}</p>
          <p className="text-xs text-muted-foreground">@{user.username}</p>
        </div>
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        onClick={onRemove}
        className="opacity-70 hover:opacity-100"
      >
        <X className="h-3 w-3" />
      </Button>
    </div>
  )
}

// Droppable Container Component
function DroppableContainer({
  id,
  children,
  className,
  label,
  count,
  color,
  editable = false,
  onNameChange,
  onColorChange,
  style,
}: {
  id: string
  children: React.ReactNode
  className?: string
  label: string
  count: number
  color?: string
  editable?: boolean
  onNameChange?: (name: string) => void
  onColorChange?: (color: string) => void
  style?: React.CSSProperties
}) {
  const { setNodeRef, isOver } = useDroppable({ id })

  return (
    <div className="space-y-2">
      {editable ? (
        <div className="space-y-2">
          <div className="flex items-center space-x-2">
            <input
              type="color"
              value={color || '#000000'}
              onChange={(e) => onColorChange?.(e.target.value)}
              className="w-8 h-8 border border-border rounded cursor-pointer"
            />
            <Input
              value={label.replace(` (${count})`, '')}
              onChange={(e) => onNameChange?.(e.target.value)}
              placeholder="Team name"
              className="flex-1"
            />
            <span className="text-sm text-muted-foreground">({count})</span>
          </div>
        </div>
      ) : (
        <Label className="flex items-center space-x-2">
          {color && <div className={`w-4 h-4 rounded`} style={{ backgroundColor: color }}></div>}
          <span>{label} ({count})</span>
        </Label>
      )}
      <div
        ref={setNodeRef}
        style={style}
        className={`space-y-2 min-h-[100px] p-3 border rounded-md transition-colors ${
          isOver ? 'border-blue-500 dark:border-blue-400 bg-blue-50 dark:bg-blue-950/20' : 'border-border'
        } ${className}`}
      >
        {children}
      </div>
    </div>
  )
}

// Helper function to generate light background color
function lightenColor(color: string, amount: number = 0.9): string {
  const hex = color.replace('#', '')
  const r = parseInt(hex.substr(0, 2), 16)
  const g = parseInt(hex.substr(2, 2), 16)
  const b = parseInt(hex.substr(4, 2), 16)
  
  return `rgba(${r}, ${g}, ${b}, ${amount})`
}

export function CreateGamePage() {
  const navigate = useNavigate()
  const { loading, error, createGame } = useCreateGame()
  const { data: searchResults, searchUsers } = useSearchUsers()
  // Form state
  const [title, setTitle] = useState('')
  const [gameType, setGameType] = useState<GameType>('football_5_a_side' as GameType)
  const [locationName, setLocationName] = useState('')
  const [latitude, setLatitude] = useState('')
  const [longitude, setLongitude] = useState('')
  const [scheduledDate, setScheduledDate] = useState('')
  const [scheduledTime, setScheduledTime] = useState('')
  const [duration, setDuration] = useState('90')
  
  // Team state
  const [useTeams, setUseTeams] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([]) // For casual mode
  const [teamAUsers, setTeamAUsers] = useState<User[]>([]) // For teams mode
  const [teamBUsers, setTeamBUsers] = useState<User[]>([]) // For teams mode
  
  // Team customization state
  const [teamAName, setTeamAName] = useState('Team A')
  const [teamBName, setTeamBName] = useState('Team B')
  const [teamAColor, setTeamAColor] = useState('#FF4444')
  const [teamBColor, setTeamBColor] = useState('#4444FF')

  // Drag and drop state
  const [activeUser, setActiveUser] = useState<User | null>(null)
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8, // Minimum distance to start dragging
      },
    })
  )

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

    // Build teams array
    const teams: CreateGameTeamInput[] = useTeams 
      ? [
          {
            name: teamAName.trim() || "Team A",
            color: teamAColor,
            invited_user_ids: teamAUsers.map(user => user.id),
          },
          {
            name: teamBName.trim() || "Team B", 
            color: teamBColor,
            invited_user_ids: teamBUsers.map(user => user.id),
          }
        ]
      : [
          {
            name: "Default",
            invited_user_ids: selectedUsers.map(user => user.id),
          }
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
      teams,
    }

    const result = await createGame(gameInput)
    if (result) {
      // Navigate to the created game's details page with URL-encoded ID
      navigate(`/games/${encodeURIComponent(result.id)}`)
    }
  }

  const addUserToCasual = (user: User) => {
    if (!selectedUsers.some(u => u.id === user.id)) {
      setSelectedUsers(prev => [...prev, user])
    }
    setSearchQuery('')
  }

  const addUserToTeamA = (user: User) => {
    if (!teamAUsers.some(u => u.id === user.id) && !teamBUsers.some(u => u.id === user.id)) {
      setTeamAUsers(prev => [...prev, user])
    }
    setSearchQuery('')
  }

  const addUserToTeamB = (user: User) => {
    if (!teamBUsers.some(u => u.id === user.id) && !teamAUsers.some(u => u.id === user.id)) {
      setTeamBUsers(prev => [...prev, user])
    }
    setSearchQuery('')
  }

  const removeUserFromCasual = (userId: string) => {
    setSelectedUsers(prev => prev.filter(user => user.id !== userId))
  }

  const removeUserFromTeamA = (userId: string) => {
    setTeamAUsers(prev => prev.filter(user => user.id !== userId))
  }

  const removeUserFromTeamB = (userId: string) => {
    setTeamBUsers(prev => prev.filter(user => user.id !== userId))
  }

  // Drag and drop handlers
  const handleDragStart = (event: DragStartEvent) => {
    const { active } = event
    const user = [...teamAUsers, ...teamBUsers, ...selectedUsers].find(u => u.id === active.id)
    setActiveUser(user || null)
  }

  const handleDragEnd = (event: DragEndEvent) => {
    const { active, over } = event
    setActiveUser(null)

    if (!over) return

    const activeUserId = active.id as string
    const overContainer = over.id as string

    // Find which container the user is currently in
    const activeUser = [...teamAUsers, ...teamBUsers, ...selectedUsers].find(u => u.id === activeUserId)
    if (!activeUser) return

    const isInTeamA = teamAUsers.some(u => u.id === activeUserId)
    const isInTeamB = teamBUsers.some(u => u.id === activeUserId)
    const isInCasual = selectedUsers.some(u => u.id === activeUserId)

    // Remove user from current container
    if (isInTeamA) {
      setTeamAUsers(prev => prev.filter(u => u.id !== activeUserId))
    } else if (isInTeamB) {
      setTeamBUsers(prev => prev.filter(u => u.id !== activeUserId))
    } else if (isInCasual) {
      setSelectedUsers(prev => prev.filter(u => u.id !== activeUserId))
    }

    // Add user to target container
    if (overContainer === 'teamA' && !teamAUsers.some(u => u.id === activeUserId)) {
      setTeamAUsers(prev => [...prev, activeUser])
    } else if (overContainer === 'teamB' && !teamBUsers.some(u => u.id === activeUserId)) {
      setTeamBUsers(prev => [...prev, activeUser])
    } else if (overContainer === 'casual' && !selectedUsers.some(u => u.id === activeUserId)) {
      setSelectedUsers(prev => [...prev, activeUser])
    } else {
      // If dropping on same container or invalid target, put user back
      if (isInTeamA) {
        setTeamAUsers(prev => [...prev, activeUser])
      } else if (isInTeamB) {
        setTeamBUsers(prev => [...prev, activeUser])
      } else if (isInCasual) {
        setSelectedUsers(prev => [...prev, activeUser])
      }
    }
  }

  const availableUsers = searchResults?.filter(user => {
    if (useTeams) {
      return !teamAUsers.some(selected => selected.id === user.id) &&
             !teamBUsers.some(selected => selected.id === user.id)
    } else {
      return !selectedUsers.some(selected => selected.id === user.id)
    }
  }) || []

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragStart={handleDragStart}
      onDragEnd={handleDragEnd}
    >
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

        {/* Team Organization */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <div className="flex items-center justify-between mb-4">
            <h3 className="text-lg font-semibold">Player Organization</h3>
            <label className="flex items-center space-x-2">
              <input
                type="checkbox"
                checked={useTeams}
                onChange={(e) => setUseTeams(e.target.checked)}
                className="w-4 h-4"
              />
              <span className="text-sm">Organize into teams</span>
            </label>
          </div>

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
                    <div key={user.id} className="p-2 hover:bg-muted border-b last:border-b-0">
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="font-medium">{user.first_name} {user.last_name}</p>
                          <p className="text-sm text-muted-foreground">@{user.username}</p>
                        </div>
                        {useTeams ? (
                          <div className="flex space-x-2">
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => addUserToTeamA(user)}
                              className="text-xs"
                            >
                              Team A
                            </Button>
                            <Button
                              type="button"
                              size="sm"
                              variant="outline"
                              onClick={() => addUserToTeamB(user)}
                              className="text-xs"
                            >
                              Team B
                            </Button>
                          </div>
                        ) : (
                          <Button
                            type="button"
                            size="sm"
                            variant="ghost"
                            onClick={() => addUserToCasual(user)}
                          >
                            <Plus className="h-4 w-4" />
                          </Button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* Teams or Casual Display with Drag & Drop */}
            {useTeams ? (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {/* Team A */}
                <DroppableContainer
                  id="teamA"
                  label={teamAName}
                  count={teamAUsers.length}
                  color={teamAColor}
                  className=""
                  style={{ backgroundColor: lightenColor(teamAColor, 0.1) }}
                  editable={true}
                  onNameChange={setTeamAName}
                  onColorChange={setTeamAColor}
                >
                  <SortableContext items={teamAUsers.map(u => u.id)} strategy={verticalListSortingStrategy}>
                    {teamAUsers.map((user) => (
                      <DraggableUser
                        key={user.id}
                        user={user}
                        onRemove={() => removeUserFromTeamA(user.id)}
                      />
                    ))}
                    {teamAUsers.length === 0 && (
                      <p className="text-sm text-muted-foreground text-center py-8">
                        Drag players here for {teamAName}
                      </p>
                    )}
                  </SortableContext>
                </DroppableContainer>

                {/* Team B */}
                <DroppableContainer
                  id="teamB"
                  label={teamBName}
                  count={teamBUsers.length}
                  color={teamBColor}
                  className=""
                  style={{ backgroundColor: lightenColor(teamBColor, 0.1) }}
                  editable={true}
                  onNameChange={setTeamBName}
                  onColorChange={setTeamBColor}
                >
                  <SortableContext items={teamBUsers.map(u => u.id)} strategy={verticalListSortingStrategy}>
                    {teamBUsers.map((user) => (
                      <DraggableUser
                        key={user.id}
                        user={user}
                        onRemove={() => removeUserFromTeamB(user.id)}
                      />
                    ))}
                    {teamBUsers.length === 0 && (
                      <p className="text-sm text-muted-foreground text-center py-8">
                        Drag players here for {teamBName}
                      </p>
                    )}
                  </SortableContext>
                </DroppableContainer>
              </div>
            ) : (
              /* Casual Mode - Simple List */
              selectedUsers.length > 0 && (
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
                          onClick={() => removeUserFromCasual(user.id)}
                        >
                          <X className="h-4 w-4" />
                        </Button>
                      </div>
                    ))}
                  </div>
                </div>
              )
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
      
      <DragOverlay>
        {activeUser ? (
          <DraggableUser
            user={activeUser}
            onRemove={() => {}}
            isDragOverlay={true}
          />
        ) : null}
      </DragOverlay>
    </DndContext>
  )
}
