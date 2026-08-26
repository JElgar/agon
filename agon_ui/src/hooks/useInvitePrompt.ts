import { useEffect, useState } from 'react'

/** localStorage key prefix — remembers that the first-open invite prompt has
 *  already been shown for a given invitation, so it doesn't reappear on
 *  every subsequent visit to the match/team page while the invite is still
 *  pending. Keyed by invitation id (stable, unique per invitee) rather than
 *  match/team id, so responding elsewhere and later being re-invited still
 *  gets a fresh prompt. */
const SEEN_PREFIX = 'agon-invite-prompt-seen:'

/**
 * Drives the "you're invited" popup that greets the viewer the first time
 * they open a match/team page they've been invited to (see
 * `InvitePromptDialog`) — auto-opens once per invitation, then stays closed
 * on later visits even if the invite is still pending, whether the viewer
 * responded or just dismissed it with the corner close button.
 *
 * `invitationId` should be the viewer's own pending invitation id for this
 * match/team, or `null` when there isn't one (not invited, already
 * responded, or not signed in) — the prompt never opens in that case.
 */
export function useInvitePrompt(
  invitationId: string | null,
): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(false)

  useEffect(() => {
    if (!invitationId) {
      setOpen(false)
      return
    }
    const key = SEEN_PREFIX + invitationId
    if (localStorage.getItem(key) === 'true') return
    localStorage.setItem(key, 'true')
    setOpen(true)
  }, [invitationId])

  return [open, setOpen]
}
