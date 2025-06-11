// Migrated to use openapi-react-query
import { fetchClient } from '@/lib/api-client'
import { useState, useCallback } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { components } from '@/types/api'

type User = components['schemas']['User']
type Group = components['schemas']['Group']

// User operations
export function useCreateUser() {
  const mutation = useMutation({
    mutationFn: async (input: components['schemas']['CreateUserInput']) => {
      const response = await fetchClient.POST('/users', { body: input })
      if (response.error) throw new Error('Failed to create user')
      return response.data
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    createUser: mutation.mutateAsync
  }
}

export function useCurrentUser() {
  const query = useQuery({
    queryKey: ['current-user'],
    queryFn: async () => {
      const response = await fetchClient.GET('/users/me')
      if (response.error) throw new Error('Failed to get current user')
      return response.data
    }
  })
  
  return {
    data: query.data,
    loading: query.isLoading,
    error: query.error?.message || null
  }
}

export function useSearchUsers() {
  const [searchData, setSearchData] = useState<User[] | null>(null)
  const [searchLoading, setSearchLoading] = useState(false)
  const [searchError, setSearchError] = useState<string | null>(null)
  
  const searchUsers = useCallback(async (query: string) => {
    if (query.trim().length < 2) {
      setSearchData([])
      return
    }
    
    setSearchLoading(true)
    setSearchError(null)
    
    try {
      const response = await fetchClient.GET('/users/search', {
        params: { query: { q: query } }
      })
      if (response.error) throw new Error('Search failed')
      setSearchData(response.data || [])
    } catch (error: any) {
      setSearchError(error.message)
    } finally {
      setSearchLoading(false)
    }
  }, [])

  return {
    data: searchData,
    loading: searchLoading,
    error: searchError,
    searchUsers
  }
}

// Group operations  
export function useCreateGroup() {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: async (input: components['schemas']['CreateGroupInput']) => {
      const response = await fetchClient.POST('/groups', { body: input })
      if (response.error) throw new Error('Failed to create group')
      return response.data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['groups'] })
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    createGroup: mutation.mutateAsync
  }
}

export function useGetGroups() {
  const query = useQuery({
    queryKey: ['groups'],
    queryFn: async () => {
      const response = await fetchClient.GET('/groups')
      if (response.error) throw new Error('Failed to get groups')
      return response.data
    }
  })
  
  return {
    data: query.data,
    loading: query.isLoading,
    error: query.error?.message || null,
    getGroups: query.refetch
  }
}

export function useGetGroup() {
  const [groupData, setGroupData] = useState<Group | null>(null)
  const [groupLoading, setGroupLoading] = useState(false)
  const [groupError, setGroupError] = useState<string | null>(null)
  
  const getGroup = useCallback(async (id: string) => {
    setGroupLoading(true)
    setGroupError(null)
    
    try {
      const response = await fetchClient.GET('/groups/{id}', {
        params: { path: { id } }
      })
      if (response.error) throw new Error('Failed to get group')
      setGroupData(response.data || null)
      return response.data
    } catch (error: any) {
      setGroupError(error.message)
      return null
    } finally {
      setGroupLoading(false)
    }
  }, [])

  return {
    data: groupData,
    loading: groupLoading,
    error: groupError,
    getGroup
  }
}

export function useAddGroupMembers() {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: async ({ groupId, input }: { groupId: string, input: components['schemas']['AddGroupMembersInput'] }) => {
      const response = await fetchClient.POST('/groups/{group_id}/members', {
        params: { path: { group_id: groupId } },
        body: input
      })
      if (response.error) throw new Error('Failed to add group members')
      return response.data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['groups'] })
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    addGroupMembers: (groupId: string, input: components['schemas']['AddGroupMembersInput']) => 
      mutation.mutateAsync({ groupId, input })
  }
}

// Game operations
export function useCreateGame() {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: async (input: components['schemas']['CreateGameInput']) => {
      const response = await fetchClient.POST('/games', { body: input })
      if (response.error) throw new Error('Failed to create game')
      return response.data
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['games'] })
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    createGame: mutation.mutateAsync
  }
}

export function useGetGames() {
  const query = useQuery({
    queryKey: ['games'],
    queryFn: async () => {
      const response = await fetchClient.GET('/games')
      if (response.error) throw new Error('Failed to get games')
      return response.data
    }
  })
  
  return {
    data: query.data,
    loading: query.isLoading,
    error: query.error?.message || null,
    getGames: query.refetch
  }
}

export function useGetGameDetails(gameId?: string) {
  const query = useQuery({
    queryKey: ['game-details', gameId],
    queryFn: async () => {
      if (!gameId) throw new Error('Game ID is required')
      const response = await fetchClient.GET('/games/{id}', {
        params: { path: { id: gameId } }
      })
      if (response.error) throw new Error('Failed to get game details')
      return response.data
    },
    enabled: !!gameId
  })
  
  return {
    data: query.data,
    loading: query.isLoading,
    error: query.error?.message || null,
    refetch: query.refetch
  }
}

export function useAddGameInvitations() {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: async ({ gameId, input }: { gameId: string, input: components['schemas']['AddGroupMembersInput'] }) => {
      const response = await fetchClient.POST('/games/{game_id}/invitations', {
        params: { path: { game_id: gameId } },
        body: input
      })
      if (response.error) throw new Error('Failed to add game invitations')
      return response.data
    },
    onSuccess: (_, variables) => {
      // Invalidate games list
      queryClient.invalidateQueries({ queryKey: ['games'] })
      // Also invalidate specific game details since new invitations were added
      queryClient.invalidateQueries({ queryKey: ['game-details', variables.gameId] })
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    addGameInvitations: (gameId: string, input: components['schemas']['AddGroupMembersInput']) => 
      mutation.mutateAsync({ gameId, input })
  }
}

export function useRespondToInvitation() {
  const queryClient = useQueryClient()
  const mutation = useMutation({
    mutationFn: async ({ gameId, userId, response }: { 
      gameId: string, 
      userId: string, 
      response: components['schemas']['InvitationResponse'] 
    }) => {
      const apiResponse = await fetchClient.PUT('/games/{game_id}/invitations/{user_id}', {
        params: { path: { game_id: gameId, user_id: userId } },
        body: { response }
      })
      if (apiResponse.error) throw new Error('Failed to respond to invitation')
      return apiResponse.data
    },
    onSuccess: (_, variables) => {
      // Invalidate games list
      queryClient.invalidateQueries({ queryKey: ['games'] })
      // Also invalidate specific game details since team membership may have changed
      queryClient.invalidateQueries({ queryKey: ['game-details', variables.gameId] })
    }
  })
  
  return {
    loading: mutation.isPending,
    error: mutation.error?.message || null,
    respondToInvitation: (gameId: string, userId: string, response: components['schemas']['InvitationResponse']) => 
      mutation.mutateAsync({ gameId, userId, response })
  }
}