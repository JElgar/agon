import { useGetGameTeams } from '@/hooks/useApi'
import { InviteLinkManager } from '@/components/InviteLinkManager'
import type { Game } from '@/lib/api'

interface GameInviteLinkButtonProps {
  game: Game
  onTokenChange?: () => void
}

export function GameInviteLinkButton({ game, onTokenChange }: GameInviteLinkButtonProps) {
  // Only show invite links for active games (not completed or cancelled)
  const shouldShowInviteLink = game.status === 'scheduled' || game.status === 'in_progress'
  
  const { data: teams, loading } = useGetGameTeams(shouldShowInviteLink ? game.id : undefined)

  // Only show invite link for active games with exactly one team
  if (!shouldShowInviteLink || loading || !teams || teams.length !== 1) {
    return null
  }

  const team = teams[0]

  return (
    <div onClick={(e) => e.stopPropagation()}>
      <InviteLinkManager
        type="team"
        id={team.id}
        currentToken={team.invite_token}
        onTokenChange={onTokenChange}
      />
    </div>
  )
}