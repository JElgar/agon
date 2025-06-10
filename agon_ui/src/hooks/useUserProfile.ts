import { useState, useCallback } from 'react'
import { api } from '@/lib/api'
import type { User } from '@/lib/api'

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
    console.log('Checking user profile...')
    setState(prev => ({ ...prev, loading: true, error: null }))
    
    try {
      // Try to get current user - if this succeeds, user profile exists
      console.log('Attempting to get current user...')
      const user = await api.getCurrentUser()
      
      console.log('Successfully got current user:', user)
      // If we get user successfully, the user profile exists
      setState({
        user,
        hasProfile: true,
        loading: false,
        error: null,
      })
      
      return true
    } catch (error: any) {
      console.log('Error getting current user:', error)
      console.log('Error status:', error?.status)
      console.log('Error message:', error?.message)
      
      // Check if it's a 404 (user not found)
      if (error?.message?.includes('404') || error?.message?.includes('User not found')) {
        console.log('User not found - showing profile creation form')
        setState({
          user: null,
          hasProfile: false,
          loading: false,
          error: null,
        })
        return false
      }
      
      // Other errors (network, auth, etc.)
      const errorMessage = error instanceof Error ? error.message : 'Failed to check user profile'
      console.log('Other error occurred:', errorMessage)
      setState({
        user: null,
        hasProfile: null,
        loading: false,
        error: errorMessage,
      })
      
      return null
    }
  }, [])

  const createUserProfile = useCallback(async (input: { email: string; first_name: string; last_name: string }) => {
    setState(prev => ({ ...prev, loading: true, error: null }))
    
    try {
      const user = await api.createUser(input)
      setState({
        user,
        hasProfile: true,
        loading: false,
        error: null,
      })
      return user
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Failed to create user profile'
      setState(prev => ({
        ...prev,
        loading: false,
        error: errorMessage,
      }))
      return null
    }
  }, [])

  const refreshProfile = useCallback(() => {
    setState({
      user: null,
      hasProfile: null,
      loading: false,
      error: null,
    })
  }, [])

  return {
    ...state,
    checkUserProfile,
    createUserProfile,
    refreshProfile,
  }
}