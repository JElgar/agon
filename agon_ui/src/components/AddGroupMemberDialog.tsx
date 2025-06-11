import { useState, useEffect, useMemo } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { useAddGroupMembers, useSearchUsers } from '@/hooks/useApi'
import { Plus, X } from 'lucide-react'
import type { User } from '@/lib/api'

interface AddGroupMemberDialogProps {
  groupId: string
  onMemberAdded?: () => void
}

export function AddGroupMemberDialog({ groupId, onMemberAdded }: AddGroupMemberDialogProps) {
  const [open, setOpen] = useState(false)
  const [searchQuery, setSearchQuery] = useState('')
  const [selectedUsers, setSelectedUsers] = useState<User[]>([])
  const { loading, error, addGroupMembers } = useAddGroupMembers()
  const { data: searchResults, searchUsers } = useSearchUsers()

  // Search users when query changes
  useEffect(() => {
    if (searchQuery.trim().length >= 2) {
      const timeoutId = setTimeout(() => {
        searchUsers(searchQuery.trim())
      }, 300)
      return () => clearTimeout(timeoutId)
    }
  }, [searchQuery, searchUsers])

  // Filter out already selected users from search results
  const availableUsers = useMemo(() => {
    if (!searchResults) return []
    return searchResults.filter(user => 
      !selectedUsers.some(selected => selected.id === user.id)
    )
  }, [searchResults, selectedUsers])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (selectedUsers.length === 0) {
      return
    }

    const userIds = selectedUsers.map(user => user.id)
    const result = await addGroupMembers(groupId, { user_ids: userIds })
    
    if (result !== null) {
      // Success - close dialog and reset form
      setOpen(false)
      setSearchQuery('')
      setSelectedUsers([])
      onMemberAdded?.()
    }
  }

  const addUser = (user: User) => {
    setSelectedUsers(prev => [...prev, user])
    setSearchQuery('') // Clear search after adding
  }

  const removeUser = (userId: string) => {
    setSelectedUsers(prev => prev.filter(user => user.id !== userId))
  }

  const handleOpenChange = (newOpen: boolean) => {
    setOpen(newOpen)
    if (!newOpen) {
      // Reset form when closing
      setSearchQuery('')
      setSelectedUsers([])
    }
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger asChild>
        <Button variant="outline">
          <Plus className="h-4 w-4 mr-2" />
          Add Member
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Add Group Members</DialogTitle>
          <DialogDescription>
            Search for users by username or name and add them to your group.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit}>
          <div className="grid gap-4 py-4">
            <div className="space-y-2">
              <Label htmlFor="search">
                Search Users
              </Label>
              <Input
                id="search"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Type username or name..."
              />
              <p className="text-xs text-muted-foreground">
                Start typing to search for users (minimum 2 characters)
              </p>
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
                <Label>Selected Members ({selectedUsers.length})</Label>
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

            {error && (
              <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
                <p className="text-destructive text-sm">{error}</p>
              </div>
            )}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => setOpen(false)}
              disabled={loading}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={loading || selectedUsers.length === 0}>
              {loading ? 'Adding...' : `Add ${selectedUsers.length} Member${selectedUsers.length !== 1 ? 's' : ''}`}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}