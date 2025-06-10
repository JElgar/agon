import { OpenAPI } from '@/api/core/OpenAPI'
import { DefaultService } from '@/api/services/DefaultService'
import { supabase } from './supabase'

// Configure the API client
export function configureApiClient() {
  // Set the base URL - you can make this configurable via env vars
  OpenAPI.BASE = import.meta.env.VITE_API_BASE_URL || 'http://localhost:7000'
  
  // Configure token resolver to get JWT from Supabase
  OpenAPI.TOKEN = async () => {
    const { data: { session } } = await supabase.auth.getSession()
    return session?.access_token || ''
  }
}

// Initialize the API client
configureApiClient()

// Manual API calls for endpoints not yet in generated client
async function getCurrentUser() {
  const response = await fetch(`${OpenAPI.BASE}/users/me`, {
    method: 'GET',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${await OpenAPI.TOKEN?.()}`,
    },
  })
  
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`)
  }
  
  return response.json()
}

async function searchUsers(query: string) {
  const response = await fetch(`${OpenAPI.BASE}/users/search?q=${encodeURIComponent(query)}`, {
    method: 'GET',
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${await OpenAPI.TOKEN?.()}`,
    },
  })
  
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${response.statusText}`)
  }
  
  return response.json()
}

// Export the API service with proper typing
export const api = {
  // User operations
  createUser: DefaultService.postUsers,
  getCurrentUser,
  searchUsers,
  
  // Team operations
  createTeam: DefaultService.postTeams,
  getTeams: DefaultService.getTeams,
  getTeam: DefaultService.getTeams1,
  addTeamMembers: DefaultService.postTeamsMembers,
  
  // Game operations
  createGame: DefaultService.postGames,
  getGames: DefaultService.getGames,
  getGame: DefaultService.getGames1,
  addGameInvitations: DefaultService.postGamesInvitations,
  respondToInvitation: DefaultService.putGamesInvitations,
}

// Export types for use in components
export type { User } from '@/api/models/User'
export type { Team } from '@/api/models/Team'
export type { TeamListItem } from '@/api/models/TeamListItem'
export type { CreateUserInput } from '@/api/models/CreateUserInput'
export type { CreateTeamInput } from '@/api/models/CreateTeamInput'
export type { AddTeamMembersInput } from '@/api/models/AddTeamMembersInput'

// Game types
export type { Game } from '@/api/models/Game'
export type { CreateGameInput } from '@/api/models/CreateGameInput'
export type { GameWithInvitations } from '@/api/models/GameWithInvitations'
export type { GameInvitation } from '@/api/models/GameInvitation'
export type { GameInvitationWithUser } from '@/api/models/GameInvitationWithUser'
export type { Location } from '@/api/models/Location'
export type { GameType } from '@/api/models/GameType'
export type { InvitationResponse } from '@/api/models/InvitationResponse'
export type { InvitationStatus } from '@/api/models/InvitationStatus'