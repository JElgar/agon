import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { ArrowLeft, Plus, X, Users } from 'lucide-react'
import { useCreateGroup, useSearchUsers, useAddGroupMembers } from '@/hooks/useApi'
import type { components } from '@/types/api'

type User = components['schemas']['User']

export function CreateGroupPage() {
  const navigate = useNavigate()
  const { loading: createLoading, error: createError, createGroup } = useCreateGroup()
  const { data: searchResults, searchUsers } = useSearchUsers()
  const { loading: membersLoading, error: membersError, addGroupMembers } = useAddGroupMembers()

  // Form state
  const [groupName, setGroupName] = useState('')
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
    
    if (!groupName.trim()) {
      return
    }

    try {
      // Create the group first
      const newGroup = await createGroup({ name: groupName.trim() })
      
      if (newGroup && selectedUsers.length > 0) {
        // Add selected users to the group
        const userIds = selectedUsers.map(user => user.id)
        await addGroupMembers(newGroup.id, { user_ids: userIds })
      }

      if (newGroup) {
        // Navigate to the created group's details page
        navigate(`/groups/${encodeURIComponent(newGroup.id)}`)
      }
    } catch (error) {
      console.error('Failed to create group:', error)
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

  // Filter out users who are already selected
  const availableUsers = searchResults?.filter(user => 
    !selectedUsers.some(selected => selected.id === user.id)
  ) || []

  const loading = createLoading || membersLoading
  const error = createError || membersError

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
        <h2 className="text-2xl font-bold">Create New Group</h2>
      </div>

      <form onSubmit={handleSubmit} className="space-y-6">
        {/* Group Details */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4 flex items-center">
            <Users className="h-5 w-5 mr-2" />
            Group Information
          </h3>
          <div className="space-y-4">
            <div>
              <Label htmlFor="groupName">Group Name</Label>
              <Input
                id="groupName"
                value={groupName}
                onChange={(e) => setGroupName(e.target.value)}
                placeholder="Enter group name"
                required
              />
            </div>
          </div>
        </div>

        {/* Member Invitation */}
        <div className="p-6 border border-border rounded-lg bg-card">
          <h3 className="text-lg font-semibold mb-4">Invite Members</h3>
          <div className="space-y-4">
            {/* User Search */}
            <div>
              <Label htmlFor="search">Search Users</Label>
              <Input
                id="search"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Type username or name to invite people..."
              />
              <p className="text-sm text-muted-foreground mt-1">
                You can add members later if you skip this step
              </p>
            </div>

            {/* Search Results */}
            {searchQuery.length >= 2 && availableUsers.length > 0 && (
              <div className="space-y-2">
                <Label>Search Results</Label>
                <div className="max-h-48 overflow-y-auto border rounded-md">
                  {availableUsers.map((user) => (
                    <div
                      key={user.id}
                      className="p-3 hover:bg-muted cursor-pointer border-b last:border-b-0"
                      onClick={() => addUser(user)}
                    >
                      <div className="flex items-center justify-between">
                        <div>
                          <p className="font-medium">{user.first_name} {user.last_name}</p>
                          <p className="text-sm text-muted-foreground">@{user.username}</p>
                          <p className="text-xs text-muted-foreground">{user.email}</p>
                        </div>
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                        >
                          <Plus className="h-4 w-4" />
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* No results message */}
            {searchQuery.length >= 2 && availableUsers.length === 0 && searchResults && searchResults.length > 0 && (
              <div className="text-sm text-muted-foreground p-3 border rounded-md">
                All found users are already selected.
              </div>
            )}

            {/* Selected Users */}
            {selectedUsers.length > 0 && (
              <div className="space-y-2">
                <Label>Selected Members ({selectedUsers.length})</Label>
                <div className="space-y-2 max-h-48 overflow-y-auto">
                  {selectedUsers.map((user) => (
                    <div
                      key={user.id}
                      className="flex items-center justify-between p-3 border border-border rounded-lg bg-muted/50"
                    >
                      <div>
                        <p className="font-medium">{user.first_name} {user.last_name}</p>
                        <p className="text-sm text-muted-foreground">@{user.username}</p>
                        <p className="text-xs text-muted-foreground">{user.email}</p>
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
            onClick={() => navigate('/groups')}
            disabled={loading}
          >
            Cancel
          </Button>
          <Button type="submit" disabled={loading || !groupName.trim()}>
            {loading ? 'Creating...' : 'Create Group'}
          </Button>
        </div>
      </form>
    </div>
  )
}