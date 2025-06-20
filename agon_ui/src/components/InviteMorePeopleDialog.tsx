import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Plus, X, UserPlus } from 'lucide-react'
import { useSearchUsers, useAddGameInvitations, useGetGroup } from '@/hooks/useApi'
import { GroupSearch } from '@/components/GroupSearch'
import { expandGroupsToUserIds, getInvitationSummary, validateInvitationData } from '@/utils/group-utils'
import type { User } from '@/lib/api'
import type { components } from '@/types/api'

type GroupListItem = components['schemas']['GroupListItem']
type GameTeam = components['schemas']['GameTeam']

interface InviteMorePeopleDialogProps {
  gameId: string
  isOpen: boolean
  onClose: () => void
  onInvitesSent: () => void
  existingInvitations: User[]
  teams: GameTeam[]
}

export function InviteMorePeopleDialog({ 
  gameId, 
  isOpen, 
  onClose, 
  onInvitesSent,
  existingInvitations,
  teams
}: InviteMorePeopleDialogProps) {
  const { data: searchResults, searchUsers } = useSearchUsers()
  const { loading: inviteLoading, error: inviteError, addGameInvitations } = useAddGameInvitations()
  const { getGroup } = useGetGroup()

  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([])
  const [selectedGroups, setSelectedGroups] = useState<GroupListItem[]>([])
  const [selectedTeamId, setSelectedTeamId] = useState<string>(teams.length > 0 ? teams[0].id : '')

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
    
    const invitationData = { individualUsers: selectedUsers, selectedGroups: selectedGroups }
    if (!validateInvitationData(invitationData) || !selectedTeamId) return

    // Expand groups to individual user IDs
    const userIds = await expandGroupsToUserIds(invitationData, getGroup)
    
    const input = {
      user_ids: userIds,
      team_id: selectedTeamId
    }

    const result = await addGameInvitations(gameId, input)
    if (result !== null) {
      // Reset form state
      setSelectedUsers([])
      setSelectedGroups([])
      setSearchQuery('')
      onInvitesSent()
      onClose()
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

  const addGroup = (group: GroupListItem) => {
    if (!selectedGroups.some(g => g.id === group.id)) {
      setSelectedGroups(prev => [...prev, group])
    }
  }

  const removeGroup = (groupId: string) => {
    setSelectedGroups(prev => prev.filter(group => group.id !== groupId))
  }

  // Filter out users who already have invitations and users who are already selected
  const availableUsers = searchResults?.filter(user => 
    !existingInvitations.some(existing => existing.id === user.id) &&
    !selectedUsers.some(selected => selected.id === user.id)
  ) || []

  if (!isOpen) return null

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-card border border-border rounded-lg p-6 w-full max-w-md mx-4">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-lg font-semibold flex items-center">
            <UserPlus className="h-5 w-5 mr-2" />
            Invite More People
          </h3>
          <Button
            variant="ghost"
            size="sm"
            onClick={onClose}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          {/* Team Selection */}
          {teams.length > 1 && (
            <div>
              <Label htmlFor="team">Assign to Team</Label>
              <select
                id="team"
                value={selectedTeamId}
                onChange={(e) => setSelectedTeamId(e.target.value)}
                className="w-full px-3 py-2 border border-input rounded-md bg-background"
                required
              >
                {teams.map((team) => (
                  <option key={team.id} value={team.id}>
                    {team.name} ({team.members.length} members)
                  </option>
                ))}
              </select>
            </div>
          )}

          {/* User Search */}
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

          {/* No available users message */}
          {searchQuery.length >= 2 && availableUsers.length === 0 && searchResults && searchResults.length > 0 && (
            <div className="text-sm text-muted-foreground p-2 border rounded-md">
              All found users are already invited to this game.
            </div>
          )}

          {/* Group Search */}
          <GroupSearch
            selectedGroups={selectedGroups}
            onGroupAdd={addGroup}
            onGroupRemove={removeGroup}
            label="Search Groups"
            placeholder="Type group name..."
          />

          {/* Selected Users */}
          {selectedUsers.length > 0 && (
            <div className="space-y-2">
              <Label>Selected Users ({selectedUsers.length})</Label>
              <div className="space-y-2 max-h-32 overflow-y-auto">
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

          {inviteError && (
            <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
              <p className="text-destructive text-sm">{inviteError}</p>
            </div>
          )}

          <div className="flex justify-end space-x-2 pt-4">
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={inviteLoading}
            >
              Cancel
            </Button>
            <Button
              type="submit"
              disabled={!validateInvitationData({ individualUsers: selectedUsers, selectedGroups: selectedGroups }) || inviteLoading}
            >
              {inviteLoading ? 'Sending...' : `Send Invitations (${getInvitationSummary({ individualUsers: selectedUsers, selectedGroups: selectedGroups })})`}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}