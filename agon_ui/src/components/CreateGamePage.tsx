import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Calendar } from '@/components/ui/calendar'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { ArrowLeft, Plus, X, GripVertical, ChevronDownIcon, RotateCcw } from 'lucide-react'
import { useCreateGame, useSearchUsers } from '@/hooks/useApi'
import { GroupSearch } from '@/components/GroupSearch'
// import { expandGroupsToUserIds, getInvitationSummary, validateInvitationData } from '@/utils/group-utils'
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
type GroupListItem = components['schemas']['GroupListItem']
type CreateGameInput = components['schemas']['CreateGameInput']
type CreateGameTeamInput = components['schemas']['CreateGameTeamInput']
type GameSchedule = components['schemas']['GameSchedule']

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

const QUICK_CRON_OPTIONS = [
  { value: '0 0 18 * * Mon *', label: 'Every Monday at 6 PM' },
  { value: '0 0 18 * * Tue *', label: 'Every Tuesday at 6 PM' },
  { value: '0 0 18 * * Wed *', label: 'Every Wednesday at 6 PM' },
  { value: '0 0 18 * * Thu *', label: 'Every Thursday at 6 PM' },
  { value: '0 0 18 * * Fri *', label: 'Every Friday at 6 PM' },
  { value: '0 0 18 * * Sat *', label: 'Every Saturday at 6 PM' },
  { value: '0 0 18 * * Sun *', label: 'Every Sunday at 6 PM' },
  { value: '0 0 19 * * Mon,Wed,Fri *', label: 'Monday, Wednesday, Friday at 7 PM' },
  { value: '0 0 20 * * Sat,Sun *', label: 'Weekends at 8 PM' },
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
  // const { getGroup } = useGetGroup() // Not used in create form
  // Form state
  const [title, setTitle] = useState('')
  const [gameType, setGameType] = useState<GameType>('football_5_a_side' as GameType)
  const [locationName, setLocationName] = useState('')
  const [latitude, setLatitude] = useState('')
  const [longitude, setLongitude] = useState('')
  const [duration, setDuration] = useState('90')
  
  // Schedule state
  const [scheduleType, setScheduleType] = useState<'one_off' | 'recurring'>('one_off')
  
  // One-off schedule fields
  const [scheduledDate, setScheduledDate] = useState<Date | undefined>(undefined)
  const [scheduledTime, setScheduledTime] = useState('')
  
  // Recurring schedule fields
  const [cronSchedule, setCronSchedule] = useState('')
  const [startDate, setStartDate] = useState<Date | undefined>(undefined)
  const [endDate, setEndDate] = useState<Date | undefined>(undefined)
  const [quickCron, setQuickCron] = useState('')
  
  // Validation state
  const [validationErrors, setValidationErrors] = useState<string[]>([])
  
  // Handle quick cron selection
  const handleQuickCronChange = (value: string) => {
    setQuickCron(value)
    if (value) {
      setCronSchedule(value)
    }
  }
  
  // Handle manual cron schedule change
  const handleCronScheduleChange = (value: string) => {
    setCronSchedule(value)
    // Clear quick cron if manually editing
    if (value !== quickCron) {
      setQuickCron('')
    }
  }
  
  // Basic cron validation for 7-field format
  const validateCronSchedule = (cron: string): string | null => {
    if (!cron.trim()) return null
    
    const parts = cron.trim().split(/\s+/)
    if (parts.length !== 7) {
      return 'Cron schedule must have exactly 7 parts (seconds minutes hours day month weekday year)'
    }
    
    // Basic format check - could be enhanced
    const [seconds, minutes, hours] = parts
    
    // Simple validation for common patterns
    const timePattern = /^(\*|\d+|\d+-\d+|\*\/\d+|\d+(,\d+)*|Mon|Tue|Wed|Thu|Fri|Sat|Sun|Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)$/i
    
    if (!timePattern.test(seconds)) {
      return 'Invalid seconds format'
    }
    if (!timePattern.test(minutes)) {
      return 'Invalid minutes format'
    }
    if (!timePattern.test(hours)) {
      return 'Invalid hours format'
    }
    
    return null
  }
  
  const cronError = validateCronSchedule(cronSchedule)
  
  // Calendar popover state
  const [calendarOpen, setCalendarOpen] = useState(false)
  const [startDateCalendarOpen, setStartDateCalendarOpen] = useState(false)
  const [endDateCalendarOpen, setEndDateCalendarOpen] = useState(false)
  
  // Team state
  const [useTeams, setUseTeams] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([]) // For casual mode
  const [selectedGroups, setSelectedGroups] = useState<GroupListItem[]>([]) // For casual mode groups
  const [teamAUsers, setTeamAUsers] = useState<User[]>([]) // For teams mode
  const [teamBUsers, setTeamBUsers] = useState<User[]>([]) // For teams mode
  const [teamAGroups, setTeamAGroups] = useState<GroupListItem[]>([]) // For teams mode groups
  const [teamBGroups, setTeamBGroups] = useState<GroupListItem[]>([]) // For teams mode groups
  
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
    
    console.log('Form submitted with data:', {
      title: title.trim(),
      latitude,
      longitude,
      scheduleType,
      scheduledDate,
      scheduledTime,
      cronSchedule: cronSchedule.trim(),
      startDate,
      endDate
    })
    
    // Clear previous validation errors
    const errors: string[] = []
    
    // Basic validation
    if (!title.trim()) errors.push('Game title is required')
    if (!latitude) errors.push('Latitude is required')
    if (!longitude) errors.push('Longitude is required')

    // Validate schedule fields based on type
    if (scheduleType === 'one_off') {
      if (!scheduledDate) errors.push('Scheduled date is required for one-time games')
      if (!scheduledTime) errors.push('Scheduled time is required for one-time games')
    } else {
      if (!cronSchedule.trim()) errors.push('Cron schedule is required for recurring games')
      if (!startDate) errors.push('Start date is required for recurring games')
      
      // Check for cron validation errors
      if (cronError) errors.push(cronError)
    }
    
    if (errors.length > 0) {
      setValidationErrors(errors)
      console.log('Validation failed:', errors)
      return
    }
    
    // Clear validation errors if we get here
    setValidationErrors([])

    // Build schedule object
    let schedule: GameSchedule
    if (scheduleType === 'one_off') {
      // Combine date and time into ISO string
      const year = scheduledDate!.getFullYear()
      const month = String(scheduledDate!.getMonth() + 1).padStart(2, '0')
      const day = String(scheduledDate!.getDate()).padStart(2, '0')
      const dateString = `${year}-${month}-${day}`
      const scheduledDateTime = new Date(`${dateString}T${scheduledTime}`).toISOString()
      
      schedule = {
        type: 'one_off',
        scheduled_time: scheduledDateTime
      }
    } else {
      // Format dates for API
      const startDateString = startDate!.toISOString().split('T')[0] // YYYY-MM-DD format
      const endDateString = endDate ? endDate.toISOString().split('T')[0] : undefined
      
      schedule = {
        type: 'recurring',
        cron_schedule: cronSchedule.trim(),
        start_date: startDateString,
        end_date: endDateString
      }
    }

    // Build teams array with user IDs and group IDs
    const teams: CreateGameTeamInput[] = useTeams 
      ? [
          {
            name: teamAName.trim() || "Team A",
            color: teamAColor,
            invited_user_ids: teamAUsers.map(user => user.id),
            invited_group_ids: teamAGroups.map(group => group.id),
          },
          {
            name: teamBName.trim() || "Team B", 
            color: teamBColor,
            invited_user_ids: teamBUsers.map(user => user.id),
            invited_group_ids: teamBGroups.map(group => group.id),
          }
        ]
      : [
          {
            name: "Default",
            invited_user_ids: selectedUsers.map(user => user.id),
            invited_group_ids: selectedGroups.map(group => group.id),
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
      schedule,
      duration_minutes: parseInt(duration),
      teams,
    }

    console.log('Calling createGame API with:', gameInput)
    
    try {
      const result = await createGame(gameInput)
      console.log('Create game result:', result)
      
      if (result) {
        // Navigate to the created game's details page with URL-encoded ID
        navigate(`/games/${encodeURIComponent(result.id)}`)
      } else {
        console.log('Create game returned null/undefined')
      }
    } catch (err) {
      console.error('Error creating game:', err)
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

  // Group handlers for casual mode
  const addGroupToCasual = (group: GroupListItem) => {
    if (!selectedGroups.some(g => g.id === group.id)) {
      setSelectedGroups(prev => [...prev, group])
    }
  }

  const removeGroupFromCasual = (groupId: string) => {
    setSelectedGroups(prev => prev.filter(group => group.id !== groupId))
  }

  // Group handlers for team A
  const addGroupToTeamA = (group: GroupListItem) => {
    if (!teamAGroups.some(g => g.id === group.id) && !teamBGroups.some(g => g.id === group.id)) {
      setTeamAGroups(prev => [...prev, group])
    }
  }

  const removeGroupFromTeamA = (groupId: string) => {
    setTeamAGroups(prev => prev.filter(group => group.id !== groupId))
  }

  // Group handlers for team B
  const addGroupToTeamB = (group: GroupListItem) => {
    if (!teamBGroups.some(g => g.id === group.id) && !teamAGroups.some(g => g.id === group.id)) {
      setTeamBGroups(prev => [...prev, group])
    }
  }

  const removeGroupFromTeamB = (groupId: string) => {
    setTeamBGroups(prev => prev.filter(group => group.id !== groupId))
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

            {/* Schedule Type Selection */}
            <div className="space-y-4">
              <div>
                <Label className="text-base font-medium">Schedule Type</Label>
                <div className="flex items-center space-x-6 mt-2">
                  <label className="flex items-center space-x-2">
                    <input
                      type="radio"
                      name="scheduleType"
                      value="one_off"
                      checked={scheduleType === 'one_off'}
                      onChange={(e) => setScheduleType(e.target.value as 'one_off' | 'recurring')}
                      className="w-4 h-4"
                    />
                    <span className="text-sm">One-time game</span>
                  </label>
                  <label className="flex items-center space-x-2">
                    <input
                      type="radio"
                      name="scheduleType"
                      value="recurring"
                      checked={scheduleType === 'recurring'}
                      onChange={(e) => setScheduleType(e.target.value as 'one_off' | 'recurring')}
                      className="w-4 h-4"
                    />
                    <span className="text-sm flex items-center">
                      <RotateCcw className="h-4 w-4 mr-1" />
                      Recurring games
                    </span>
                  </label>
                </div>
              </div>
              
              {/* One-off Schedule */}
              {scheduleType === 'one_off' && (
                <div className="flex gap-4">
                  <div className="flex flex-col gap-3">
                    <Label htmlFor="date" className="px-1">
                      Date
                    </Label>
                    <Popover open={calendarOpen} onOpenChange={setCalendarOpen}>
                      <PopoverTrigger asChild>
                        <Button
                          variant="outline"
                          id="date"
                          className="w-48 justify-between font-normal"
                        >
                          {scheduledDate ? scheduledDate.toLocaleDateString() : "Select date"}
                          <ChevronDownIcon />
                        </Button>
                      </PopoverTrigger>
                      <PopoverContent className="w-auto overflow-hidden p-0" align="start">
                        <Calendar
                          mode="single"
                          selected={scheduledDate}
                          captionLayout="dropdown"
                          onSelect={(date) => {
                            setScheduledDate(date)
                            setCalendarOpen(false)
                          }}
                        />
                      </PopoverContent>
                    </Popover>
                  </div>
                  <div className="flex flex-col gap-3">
                    <Label htmlFor="time" className="px-1">
                      Time
                    </Label>
                    <Input
                      type="time"
                      id="time"
                      value={scheduledTime}
                      onChange={(e) => setScheduledTime(e.target.value)}
                      required={scheduleType === 'one_off'}
                      className="bg-background appearance-none [&::-webkit-calendar-picker-indicator]:hidden [&::-webkit-calendar-picker-indicator]:appearance-none"
                    />
                  </div>
                </div>
              )}
              
              {/* Recurring Schedule */}
              {scheduleType === 'recurring' && (
                <div className="space-y-4">
                  <div>
                    <Label htmlFor="quickCron">Quick Schedule Options</Label>
                    <select
                      id="quickCron"
                      value={quickCron}
                      onChange={(e) => handleQuickCronChange(e.target.value)}
                      className="w-full px-3 py-2 border border-input rounded-md bg-background"
                    >
                      <option value="">Select a common schedule...</option>
                      {QUICK_CRON_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.label}
                        </option>
                      ))}
                    </select>
                  </div>
                  
                  <div>
                    <Label htmlFor="cronSchedule">Custom Cron Schedule</Label>
                    <Input
                      id="cronSchedule"
                      value={cronSchedule}
                      onChange={(e) => handleCronScheduleChange(e.target.value)}
                      placeholder="e.g., 0 0 18 * * Mon * (Every Monday at 6 PM)"
                      required={scheduleType === 'recurring'}
                      className={cronError ? 'border-destructive' : ''}
                    />
                    {cronError && (
                      <p className="text-xs text-destructive mt-1">{cronError}</p>
                    )}
                    <p className="text-xs text-muted-foreground mt-1">
                      Format: seconds minutes hours day month weekday year (use * for any, Mon/Tue/etc for weekdays)
                    </p>
                  </div>
                  
                  <div className="flex gap-4">
                    <div className="flex flex-col gap-3">
                      <Label htmlFor="startDate" className="px-1">
                        Start Date
                      </Label>
                      <Popover open={startDateCalendarOpen} onOpenChange={setStartDateCalendarOpen}>
                        <PopoverTrigger asChild>
                          <Button
                            variant="outline"
                            id="startDate"
                            className="w-48 justify-between font-normal"
                          >
                            {startDate ? startDate.toLocaleDateString() : "Select start date"}
                            <ChevronDownIcon />
                          </Button>
                        </PopoverTrigger>
                        <PopoverContent className="w-auto overflow-hidden p-0" align="start">
                          <Calendar
                            mode="single"
                            selected={startDate}
                            captionLayout="dropdown"
                            onSelect={(date) => {
                              setStartDate(date)
                              setStartDateCalendarOpen(false)
                            }}
                          />
                        </PopoverContent>
                      </Popover>
                    </div>
                    
                    <div className="flex flex-col gap-3">
                      <Label htmlFor="endDate" className="px-1">
                        End Date (Optional)
                      </Label>
                      <Popover open={endDateCalendarOpen} onOpenChange={setEndDateCalendarOpen}>
                        <PopoverTrigger asChild>
                          <Button
                            variant="outline"
                            id="endDate"
                            className="w-48 justify-between font-normal"
                          >
                            {endDate ? endDate.toLocaleDateString() : "Select end date"}
                            <ChevronDownIcon />
                          </Button>
                        </PopoverTrigger>
                        <PopoverContent className="w-auto overflow-hidden p-0" align="start">
                          <Calendar
                            mode="single"
                            selected={endDate}
                            captionLayout="dropdown"
                            onSelect={(date) => {
                              setEndDate(date)
                              setEndDateCalendarOpen(false)
                            }}
                            disabled={(date) => startDate ? date < startDate : false}
                          />
                        </PopoverContent>
                      </Popover>
                      {endDate && (
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          onClick={() => setEndDate(undefined)}
                          className="text-xs"
                        >
                          Clear end date
                        </Button>
                      )}
                    </div>
                  </div>
                </div>
              )}
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

            {/* Group Search */}
            {useTeams ? (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {/* Team A Groups */}
                <div className="space-y-4">
                  <h4 className="font-medium">Team A Groups</h4>
                  <GroupSearch
                    selectedGroups={teamAGroups}
                    onGroupAdd={addGroupToTeamA}
                    onGroupRemove={removeGroupFromTeamA}
                    excludeGroups={teamBGroups}
                    label="Search Groups for Team A"
                    placeholder="Type group name for Team A..."
                  />
                </div>
                
                {/* Team B Groups */}
                <div className="space-y-4">
                  <h4 className="font-medium">Team B Groups</h4>
                  <GroupSearch
                    selectedGroups={teamBGroups}
                    onGroupAdd={addGroupToTeamB}
                    onGroupRemove={removeGroupFromTeamB}
                    excludeGroups={teamAGroups}
                    label="Search Groups for Team B"
                    placeholder="Type group name for Team B..."
                  />
                </div>
              </div>
            ) : (
              /* Casual Mode Groups */
              <GroupSearch
                selectedGroups={selectedGroups}
                onGroupAdd={addGroupToCasual}
                onGroupRemove={removeGroupFromCasual}
                label="Search Groups"
                placeholder="Type group name..."
              />
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

        {validationErrors.length > 0 && (
          <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
            <p className="text-destructive text-sm font-medium mb-2">Please fix the following errors:</p>
            <ul className="text-destructive text-sm list-disc list-inside space-y-1">
              {validationErrors.map((error, index) => (
                <li key={index}>{error}</li>
              ))}
            </ul>
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
