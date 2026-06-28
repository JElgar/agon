import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { Copy, Share, RefreshCw, Trash2 } from 'lucide-react'
import { 
  useGenerateGroupInviteToken, 
  useClearGroupInviteToken,
  useGenerateTeamInviteToken,
  useClearTeamInviteToken
} from '@/hooks/useApi'

interface InviteLinkManagerProps {
  type: 'group' | 'team'
  id: string
  currentToken?: string
  onTokenChange?: () => void
}

export function InviteLinkManager({ type, id, currentToken, onTokenChange }: InviteLinkManagerProps) {
  const [open, setOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  const [localToken, setLocalToken] = useState<string | undefined>(currentToken)

  const isGroup = type === 'group'

  // Sync local token with prop when prop changes
  useEffect(() => {
    setLocalToken(currentToken)
  }, [currentToken])

  // Use local token if available, otherwise fall back to prop
  const activeToken = localToken || currentToken
  
  const { 
    loading: generatingGroup, 
    error: generateGroupError, 
    generateToken: generateGroupToken 
  } = useGenerateGroupInviteToken()

  const { 
    loading: clearingGroup, 
    error: clearGroupError, 
    clearToken: clearGroupToken 
  } = useClearGroupInviteToken()

  const { 
    loading: generatingTeam, 
    error: generateTeamError, 
    generateToken: generateTeamToken 
  } = useGenerateTeamInviteToken()

  const { 
    loading: clearingTeam, 
    error: clearTeamError, 
    clearToken: clearTeamToken 
  } = useClearTeamInviteToken()

  const loading = isGroup ? (generatingGroup || clearingGroup) : (generatingTeam || clearingTeam)
  const error = isGroup ? (generateGroupError || clearGroupError) : (generateTeamError || clearTeamError)

  const handleGenerateToken = async () => {
    try {
      let newToken: string
      if (isGroup) {
        newToken = await generateGroupToken(id)
      } else {
        newToken = await generateTeamToken(id)
      }
      // Update local token immediately to keep dialog open with new content
      setLocalToken(newToken)
      // Delay onTokenChange to allow the dialog to remain open
      setTimeout(() => {
        onTokenChange?.()
      }, 100)
    } catch (err) {
      console.error('Failed to generate token:', err)
    }
  }

  const handleClearToken = async () => {
    try {
      if (isGroup) {
        await clearGroupToken(id)
      } else {
        await clearTeamToken(id)
      }
      // Update local token immediately
      setLocalToken(undefined)
      // Delay onTokenChange to allow the dialog to remain open
      setTimeout(() => {
        onTokenChange?.()
      }, 100)
    } catch (err) {
      console.error('Failed to clear token:', err)
    }
  }

  const inviteUrl = activeToken ? `${window.location.origin}/invite/${activeToken}?type=${type}` : ''

  const handleCopyLink = async () => {
    if (inviteUrl) {
      try {
        await navigator.clipboard.writeText(inviteUrl)
        setCopied(true)
        setTimeout(() => setCopied(false), 2000)
      } catch (err) {
        console.error('Failed to copy link:', err)
      }
    }
  }

  const handleShare = async () => {
    if (inviteUrl && navigator.share !== undefined) {
      try {
        await navigator.share({
          title: `Join ${type} on Agon`,
          text: `You've been invited to join a ${type} on Agon!`,
          url: inviteUrl
        })
      } catch (err) {
        console.error('Failed to share:', err)
      }
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm">
          <Share className="h-4 w-4 mr-2" />
          Invite Link
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {type === 'group' ? 'Group' : 'Team'} Invite Link
          </DialogTitle>
        </DialogHeader>
        <div className="space-y-4">
          {error && (
            <div className="p-3 bg-destructive/10 border border-destructive/20 rounded-md">
              <p className="text-sm text-destructive">{error}</p>
            </div>
          )}
          
          {activeToken ? (
            <div className="space-y-3">
              <div>
                <Label htmlFor="invite-link">Invite Link</Label>
                <div className="flex space-x-2 mt-1">
                  <Input
                    id="invite-link"
                    value={inviteUrl}
                    readOnly
                    className="flex-1"
                  />
                  <Button
                    size="sm"
                    onClick={handleCopyLink}
                    disabled={!inviteUrl}
                  >
                    <Copy className="h-4 w-4" />
                    {copied ? 'Copied!' : 'Copy'}
                  </Button>
                </div>
              </div>
              
              <div className="flex space-x-2">
                {navigator.share !== undefined && (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleShare}
                    disabled={!inviteUrl}
                  >
                    <Share className="h-4 w-4 mr-2" />
                    Share
                  </Button>
                )}
                
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleGenerateToken}
                  disabled={loading}
                >
                  <RefreshCw className="h-4 w-4 mr-2" />
                  {loading ? 'Generating...' : 'Regenerate'}
                </Button>
                
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleClearToken}
                  disabled={loading}
                >
                  <Trash2 className="h-4 w-4 mr-2" />
                  {loading ? 'Clearing...' : 'Clear'}
                </Button>
              </div>
              
              <p className="text-xs text-muted-foreground">
                Anyone with this link can join your {type}. Keep it secure!
              </p>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-sm text-muted-foreground">
                No invite link has been generated yet. Create one to allow others to join your {type}.
              </p>
              
              <Button
                onClick={handleGenerateToken}
                disabled={loading}
                className="w-full"
              >
                {loading ? 'Generating...' : `Generate Invite Link`}
              </Button>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}