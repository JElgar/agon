import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Plus, X, UserPlus } from 'lucide-react'
import { useSearchUsers, useAddGameInvitations } from '@/hooks/useApi'
import type { User, AddTeamMembersInput } from '@/lib/api'

interface InviteMorePeopleDialogProps {
  gameId: string
  isOpen: boolean
  onClose: () => void
  onInvitesSent: () => void
  existingInvitations: User[]
}

export function InviteMorePeopleDialog({ 
  gameId, 
  isOpen, 
  onClose, 
  onInvitesSent,
  existingInvitations 
}: InviteMorePeopleDialogProps) {
  const { data: searchResults, searchUsers } = useSearchUsers()
  const { loading: inviteLoading, error: inviteError, addGameInvitations } = useAddGameInvitations()

  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([])

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
    
    if (selectedUsers.length === 0) return

    const input: AddTeamMembersInput = {
      user_ids: selectedUsers.map(user => user.id)
    }

    const result = await addGameInvitations(gameId, input)
    if (result !== null) {
      // Reset form state
      setSelectedUsers([])
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
              disabled={selectedUsers.length === 0 || inviteLoading}
            >
              {inviteLoading ? 'Sending...' : `Send ${selectedUsers.length} Invitation${selectedUsers.length === 1 ? '' : 's'}`}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}