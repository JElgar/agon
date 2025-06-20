import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Plus, X, Users } from 'lucide-react'
import { useSearchGroups } from '@/hooks/useApi'
import type { components } from '@/types/api'

type GroupListItem = components['schemas']['GroupListItem']

interface GroupSearchProps {
  selectedGroups: GroupListItem[]
  onGroupAdd: (group: GroupListItem) => void
  onGroupRemove: (groupId: string) => void
  excludeGroups?: GroupListItem[]
  label?: string
  placeholder?: string
}

export function GroupSearch({ 
  selectedGroups, 
  onGroupAdd, 
  onGroupRemove,
  excludeGroups = [],
  label = "Search Groups",
  placeholder = "Type group name..."
}: GroupSearchProps) {
  const { data: searchResults, searchGroups } = useSearchGroups()
  const [searchQuery, setSearchQuery] = useState('')

  // Search groups when query changes
  useEffect(() => {
    if (searchQuery.trim().length >= 2) {
      const timeoutId = setTimeout(() => {
        searchGroups(searchQuery.trim())
      }, 300)
      return () => clearTimeout(timeoutId)
    }
  }, [searchQuery, searchGroups])

  const addGroup = (group: GroupListItem) => {
    if (!selectedGroups.some(g => g.id === group.id)) {
      onGroupAdd(group)
    }
    setSearchQuery('')
  }

  const removeGroup = (groupId: string) => {
    onGroupRemove(groupId)
  }

  // Filter out groups that are already selected or excluded
  const availableGroups = searchResults?.filter(group => 
    !selectedGroups.some(selected => selected.id === group.id) &&
    !excludeGroups.some(excluded => excluded.id === group.id)
  ) || []

  return (
    <div className="space-y-4">
      {/* Group Search */}
      <div>
        <Label htmlFor="group-search">{label}</Label>
        <Input
          id="group-search"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          placeholder={placeholder}
        />
      </div>

      {/* Search Results */}
      {searchQuery.length >= 2 && availableGroups.length > 0 && (
        <div className="space-y-2">
          <Label>Search Results</Label>
          <div className="max-h-32 overflow-y-auto border rounded-md">
            {availableGroups.map((group) => (
              <div
                key={group.id}
                className="p-2 hover:bg-muted cursor-pointer border-b last:border-b-0"
                onClick={() => addGroup(group)}
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center space-x-2">
                    <Users className="h-4 w-4 text-muted-foreground" />
                    <p className="font-medium">{group.name}</p>
                  </div>
                  <Plus className="h-4 w-4 text-muted-foreground" />
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* No available groups message */}
      {searchQuery.length >= 2 && availableGroups.length === 0 && searchResults && searchResults.length > 0 && (
        <div className="text-sm text-muted-foreground p-2 border rounded-md">
          All found groups are already selected.
        </div>
      )}

      {/* Selected Groups */}
      {selectedGroups.length > 0 && (
        <div className="space-y-2">
          <Label>Selected Groups ({selectedGroups.length})</Label>
          <div className="space-y-2 max-h-32 overflow-y-auto">
            {selectedGroups.map((group) => (
              <div
                key={group.id}
                className="flex items-center justify-between p-2 border rounded-md bg-muted/50"
              >
                <div className="flex items-center space-x-2">
                  <Users className="h-4 w-4 text-muted-foreground" />
                  <p className="font-medium">{group.name}</p>
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => removeGroup(group.id)}
                >
                  <X className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}