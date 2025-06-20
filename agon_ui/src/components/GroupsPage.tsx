import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { useGetGroups } from '@/hooks/useApi'
import { debugJwt } from '@/utils/jwt-debug'

export function GroupsPage() {
  const navigate = useNavigate()
  const { data: groups, loading: groupsLoading, error: groupsError } = useGetGroups()

  useEffect(() => {
    debugJwt() // Debug JWT token
  }, [])

  const handleCreateGroup = () => {
    navigate('/groups/create')
  }

  if (groupsLoading) {
    return (
      <div className="flex items-center justify-center p-8">
        <div>Loading groups...</div>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h2 className="text-2xl font-bold">Groups</h2>
        <Button onClick={handleCreateGroup}>
          Create Group
        </Button>
      </div>

      {groupsError && (
        <div className="p-4 bg-destructive/10 border border-destructive/20 rounded-md">
          <p className="text-destructive">Error loading groups: {groupsError}</p>
        </div>
      )}

      <div className="grid gap-4">
        {groups && groups.length > 0 ? (
          groups.map((group) => (
            <div 
              key={group.id}
              className="p-4 border border-border rounded-lg hover:bg-muted/50 cursor-pointer transition-colors"
              onClick={() => navigate(`/groups/${group.id}`)}
            >
              <h3 className="font-semibold">{group.name}</h3>
              <p className="text-sm text-muted-foreground">ID: {group.id}</p>
              <p className="text-xs text-muted-foreground mt-2">Click to view details</p>
            </div>
          ))
        ) : (
          <div className="text-center py-8 text-muted-foreground">
            No groups found. Create your first group!
          </div>
        )}
      </div>
    </div>
  )
}