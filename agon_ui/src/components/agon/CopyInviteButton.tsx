import { useEffect, useState } from 'react'
import { Check, Link2 } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { inviteLink } from '@/lib/members'
import { cn } from '@/lib/utils'

/**
 * Copies a pending token-invite's shareable link to the clipboard (with a
 * native share-sheet fallback on devices that support it), showing a transient
 * "Copied!" confirmation. Rendered next to external, not-yet-accepted players so
 * the inviter can hand out the `/invite/:token` link.
 */
export function CopyInviteButton({
  token,
  className,
}: {
  token: string
  className?: string
}) {
  const [copied, setCopied] = useState(false)

  // Auto-reset the "Copied!" label so the button returns to its default state.
  useEffect(() => {
    if (!copied) return
    const id = setTimeout(() => setCopied(false), 2000)
    return () => clearTimeout(id)
  }, [copied])

  const share = async () => {
    const url = inviteLink(token)
    // Prefer the native share sheet on mobile; fall back to clipboard copy.
    if (navigator.share) {
      try {
        await navigator.share({ title: 'Join me on Agon', url })
        return
      } catch {
        // User dismissed the sheet, or share failed — fall through to copy.
      }
    }
    try {
      await navigator.clipboard.writeText(url)
      setCopied(true)
    } catch {
      // Clipboard blocked (e.g. insecure context) — surface the link so the
      // user can copy it manually rather than failing silently.
      window.prompt('Copy this invite link:', url)
    }
  }

  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className={cn('h-7 gap-1 px-2 text-xs', className)}
      onClick={share}
    >
      {copied ? (
        <>
          <Check className="size-3" /> Copied!
        </>
      ) : (
        <>
          <Link2 className="size-3" /> Copy invite
        </>
      )}
    </Button>
  )
}
