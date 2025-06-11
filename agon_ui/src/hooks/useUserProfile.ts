import { useState, useCallback } from 'react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type User = components['schemas']['User']
type CreateUserInput = components['schemas']['CreateUserInput']

interface UserProfileState {
  user: User | null
  hasProfile: boolean | null // null = checking, true = has profile, false = no profile
  loading: boolean
  error: string | null
}

export function useUserProfile() {
  const [state, setState] = useState<UserProfileState>({
    user: null,
    hasProfile: null,
    loading: false,
    error: null,
  })

  const checkUserProfile = useCallback(async () => {
    setState(prev => ({ ...prev, loading: true, error: null }))
    
    const response = await fetchClient.GET('/users/me')
    
    if (response.data) {
      // User exists
      setState({
        user: response.data,
        hasProfile: true,
        loading: false,
        error: null,
      })
      return true
    }
    
    if (response.response?.status === 404) {
      // User doesn't exist - need to create profile
      setState({
        user: null,
        hasProfile: false,
        loading: false,
        error: null,
      })
      return false
    }
    
    // Other error (network, auth, etc.)
    setState({
      user: null,
      hasProfile: null,
      loading: false,
      error: response.error?.toString() || 'Failed to check user profile',
    })
    return null
  }, [])

  const createUserProfile = useCallback(async (input: CreateUserInput) => {
    setState(prev => ({ ...prev, loading: true, error: null }))
    
    const response = await fetchClient.POST('/users', { body: input })
    
    if (response.data) {
      setState({
        user: response.data,
        hasProfile: true,
        loading: false,
        error: null,
      })
      return response.data
    }
    
    setState(prev => ({
      ...prev,
      loading: false,
      error: 'Failed to create user profile',
    }))
    return null
  }, [])

  return {
    ...state,
    checkUserProfile,
    createUserProfile,
  }
}