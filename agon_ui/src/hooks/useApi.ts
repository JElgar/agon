import { useState, useCallback } from 'react'
import { api } from '@/lib/api'
import type { 
  Team, TeamListItem, User, CreateTeamInput, CreateUserInput, AddTeamMembersInput,
  Game, CreateGameInput, GameWithInvitations, InvitationResponse
} from '@/lib/api'

interface ApiState<T> {
  data: T | null
  loading: boolean
  error: string | null
}

export function useApiState<T>(): [ApiState<T>, (promise: Promise<T>) => Promise<T | null>] {
  const [state, setState] = useState<ApiState<T>>({
    data: null,
    loading: false,
    error: null,
  })

  const execute = useCallback(async (promise: Promise<T>) => {
    setState({ data: null, loading: true, error: null })
    
    try {
      const result = await promise
      setState({ data: result, loading: false, error: null })
      return result
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'An error occurred'
      setState({ data: null, loading: false, error: errorMessage })
      return null
    }
  }, [])

  return [state, execute]
}

// Specific API hooks
export function useCreateUser() {
  const [state, execute] = useApiState<User>()
  
  const createUser = useCallback((input: CreateUserInput) => 
    execute(api.createUser(input)), [execute])
  
  return { ...state, createUser }
}

export function useCreateTeam() {
  const [state, execute] = useApiState<Team>()
  
  const createTeam = useCallback((input: CreateTeamInput) => 
    execute(api.createTeam(input)), [execute])
  
  return { ...state, createTeam }
}

export function useGetTeams() {
  const [state, execute] = useApiState<TeamListItem[]>()
  
  const getTeams = useCallback(() => 
    execute(api.getTeams()), [execute])
  
  return { ...state, getTeams }
}

export function useGetTeam() {
  const [state, execute] = useApiState<Team>()
  
  const getTeam = useCallback((id: string) => 
    execute(api.getTeam(id)), [execute])
  
  return { ...state, getTeam }
}

export function useAddTeamMembers() {
  const [state, execute] = useApiState<any>()
  
  const addTeamMembers = useCallback((teamId: string, input: AddTeamMembersInput) => 
    execute(api.addTeamMembers(teamId, input)), [execute])
  
  return { ...state, addTeamMembers }
}

export function useSearchUsers() {
  const [state, execute] = useApiState<User[]>()
  
  const searchUsers = useCallback((query: string) => 
    execute(api.searchUsers(query)), [execute])
  
  return { ...state, searchUsers }
}

// Game hooks
export function useCreateGame() {
  const [state, execute] = useApiState<Game>()
  
  const createGame = useCallback((input: CreateGameInput) => 
    execute(api.createGame(input)), [execute])
  
  return { ...state, createGame }
}

export function useGetGames() {
  const [state, execute] = useApiState<Game[]>()
  
  const getGames = useCallback(() => 
    execute(api.getGames()), [execute])
  
  return { ...state, getGames }
}

export function useGetGameDetails() {
  const [state, execute] = useApiState<GameWithInvitations>()
  
  const getGameDetails = useCallback((gameId: string) => 
    execute(api.getGame(gameId)), [execute])
  
  return { ...state, getGameDetails }
}

export function useAddGameInvitations() {
  const [state, execute] = useApiState<any>()
  
  const addGameInvitations = useCallback((gameId: string, input: AddTeamMembersInput) => 
    execute(api.addGameInvitations(gameId, input)), [execute])
  
  return { ...state, addGameInvitations }
}

export function useRespondToInvitation() {
  const [state, execute] = useApiState<any>()
  
  const respondToInvitation = useCallback((gameId: string, userId: string, response: InvitationResponse) => 
    execute(api.respondToInvitation(gameId, userId, { response })), [execute])
  
  return { ...state, respondToInvitation }
}