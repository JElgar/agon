import type { components } from '@/types/api'

type User = components['schemas']['User']
type Group = components['schemas']['Group']
type GroupListItem = components['schemas']['GroupListItem']

interface GroupInvitationData {
  individualUsers: User[]
  selectedGroups: GroupListItem[]
}

/**
 * Expands groups to their individual members and deduplicates user IDs
 */
export async function expandGroupsToUserIds(
  data: GroupInvitationData,
  getGroupDetails: (groupId: string) => Promise<Group | null>
): Promise<string[]> {
  const allUserIds = new Set<string>()
  
  // Add individual users
  data.individualUsers.forEach(user => {
    allUserIds.add(user.id)
  })
  
  // Expand groups to their members
  for (const group of data.selectedGroups) {
    try {
      const groupDetails = await getGroupDetails(group.id)
      if (groupDetails?.members) {
        groupDetails.members.forEach(member => {
          allUserIds.add(member.id)
        })
      }
    } catch (error) {
      console.error(`Failed to expand group ${group.name}:`, error)
      // Continue with other groups even if one fails
    }
  }
  
  return Array.from(allUserIds)
}

/**
 * Gets a summary of what will be invited (for display purposes)
 */
export function getInvitationSummary(data: GroupInvitationData): string {
  const parts: string[] = []
  
  if (data.individualUsers.length > 0) {
    parts.push(`${data.individualUsers.length} individual user${data.individualUsers.length === 1 ? '' : 's'}`)
  }
  
  if (data.selectedGroups.length > 0) {
    parts.push(`${data.selectedGroups.length} group${data.selectedGroups.length === 1 ? '' : 's'}`)
  }
  
  return parts.join(' and ')
}

/**
 * Validates that at least some users or groups are selected
 */
export function validateInvitationData(data: GroupInvitationData): boolean {
  return data.individualUsers.length > 0 || data.selectedGroups.length > 0
}