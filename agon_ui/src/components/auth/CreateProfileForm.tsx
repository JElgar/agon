import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { $api } from '@/lib/api-client'
import { useAuth } from '@/hooks/useAuth'

interface CreateProfileFormProps {
  /** The verified account email (from Supabase). Shown read-only; the API takes
   *  the email from the JWT, not this form. */
  email: string
  onProfileCreated: () => void
}

/** Best-effort display name from the Supabase identity (Google OAuth etc.). */
function suggestedName(user: ReturnType<typeof useAuth>['user']): string {
  const profileData = {
    ...user?.user_metadata,
    ...user?.identities?.[0]?.identity_data,
  }
  return profileData.name || profileData.full_name || ''
}

export function CreateProfileForm({ email, onProfileCreated }: CreateProfileFormProps) {
  const { user } = useAuth()
  const [name, setName] = useState('')

  const createUser = $api.useMutation('post', '/users', {
    onSuccess: () => onProfileCreated(),
  })

  // Pre-fill the name from the OAuth identity, if present.
  useEffect(() => {
    const suggestion = suggestedName(user)
    if (suggestion) setName(suggestion)
  }, [user])

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    createUser.mutate({ body: { name: name.trim() } })
  }

  return (
    <div className="w-full max-w-md mx-auto p-6">
      <div className="text-center mb-6">
        <h2 className="text-2xl font-bold mb-2">Complete your profile</h2>
        <p className="text-muted-foreground">
          Choose the display name others will see.
        </p>
      </div>

      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <Label htmlFor="email">Email</Label>
          <Input
            id="email"
            type="email"
            value={email}
            disabled
            className="bg-muted text-muted-foreground"
          />
        </div>

        <div>
          <Label htmlFor="name">Display name</Label>
          <Input
            id="name"
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Enter your name"
            required
          />
        </div>

        {createUser.isError && (
          <div className="p-4 bg-red-50 border border-red-200 rounded-md">
            <p className="text-red-800 text-sm">
              {createUser.error?.toString() || 'Failed to create profile'}
            </p>
          </div>
        )}

        <Button
          type="submit"
          disabled={createUser.isPending || !name.trim()}
          className="w-full"
        >
          {createUser.isPending ? 'Creating profile…' : 'Create profile'}
        </Button>
      </form>
    </div>
  )
}
