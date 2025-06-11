// Re-export types from the new API client for backward compatibility
import type { components } from '@/types/api'
export type { components } from '@/types/api'

// Export specific types that components might still be importing
export type User = components['schemas']['User']
export type Group = components['schemas']['Group']
export type GroupListItem = components['schemas']['GroupListItem']
export type Game = components['schemas']['Game']
export type GameWithInvitations = components['schemas']['GameWithInvitations']
export type GameType = components['schemas']['GameType']
export type CreateGameInput = components['schemas']['CreateGameInput']
export type CreateGameTeamInput = components['schemas']['CreateGameTeamInput']
export type GameTeam = components['schemas']['GameTeam']
export type InvitationResponse = components['schemas']['InvitationResponse']
export type InvitationStatus = components['schemas']['InvitationStatus']
export type AddGroupMembersInput = components['schemas']['AddGroupMembersInput']
export type CreateUserInput = components['schemas']['CreateUserInput']

// Re-export the API client
export { $api } from '@/lib/api-client'