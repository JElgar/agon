import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { $api } from '@/lib/api-client'
import { useAuth } from '@/hooks/useAuth'
import { joinName, splitName } from '@/lib/names'

interface CreateProfileFormProps {
  /** The verified account email (from Supabase). Shown read-only; the API takes
   *  the email from the JWT, not this form. */
  email: string
  onProfileCreated: () => void
}

/** Best-effort first/last name from the Supabase identity (Google OAuth etc.). */
function suggestedName(user: ReturnType<typeof useAuth>['user']): { firstName: string; lastName: string } {
  const profileData = {
    ...user?.user_metadata,
    ...user?.identities?.[0]?.identity_data,
  }
  if (profileData.given_name || profileData.family_name) {
    return { firstName: profileData.given_name || '', lastName: profileData.family_name || '' }
  }
  const fullName = profileData.name || profileData.full_name || ''
  return splitName(fullName)
}

export function CreateProfileForm({ email, onProfileCreated }: CreateProfileFormProps) {
  const { user } = useAuth()
  const [firstName, setFirstName] = useState('')
  const [lastName, setLastName] = useState('')

  const createUser = $api.useMutation('post', '/users', {
    onSuccess: () => onProfileCreated(),
  })

  // Pre-fill the name from the OAuth identity, if present.
  useEffect(() => {
    const suggestion = suggestedName(user)
    if (suggestion.firstName) setFirstName(suggestion.firstName)
    if (suggestion.lastName) setLastName(suggestion.lastName)
  }, [user])

  const canSubmit = firstName.trim() !== '' && lastName.trim() !== ''

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    createUser.mutate({ body: { name: joinName(firstName, lastName) } })
  }

  return (
    <div className="w-full max-w-md mx-auto p-6">
      <div className="text-center mb-6">
        <h2 className="text-2xl font-bold mb-2">Complete your profile</h2>
        <p className="text-muted-foreground">
          Tell us your name so others can recognize you.
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

        <div className="grid grid-cols-2 gap-4">
          <div>
            <Label htmlFor="first-name">First name</Label>
            <Input
              id="first-name"
              type="text"
              value={firstName}
              onChange={(e) => setFirstName(e.target.value)}
              placeholder="First name"
              required
            />
          </div>

          <div>
            <Label htmlFor="last-name">Last name</Label>
            <Input
              id="last-name"
              type="text"
              value={lastName}
              onChange={(e) => setLastName(e.target.value)}
              placeholder="Last name"
              required
            />
          </div>
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
          disabled={createUser.isPending || !canSubmit}
          className="w-full"
        >
          {createUser.isPending ? 'Creating profile…' : 'Create profile'}
        </Button>
      </form>
    </div>
  )
}
