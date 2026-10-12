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
      {/* Wordmark + tagline */}
      <div className="flex flex-col items-center gap-1.5 mb-9">
        <span className="font-serif text-4xl font-semibold italic leading-none text-foreground">
          Agon
        </span>
        <span className="text-sm text-muted-foreground">
          Track every match. Never miss a game.
        </span>
      </div>

      <div className="space-y-3.5">
        <div className="text-center">
          <h2 className="font-display text-xl font-bold text-foreground">
            Complete your profile
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Tell us your name so others can recognize you.
          </p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-3.5">
          <div className="space-y-1.5">
            <Label htmlFor="email" className="pl-0.5 text-[13px] font-bold text-foreground/85">
              Email
            </Label>
            <Input
              id="email"
              type="email"
              value={email}
              disabled
              className="h-12 rounded-2xl px-3.5 text-[15px] font-medium bg-muted text-muted-foreground"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="first-name" className="pl-0.5 text-[13px] font-bold text-foreground/85">
                First name
              </Label>
              <Input
                id="first-name"
                type="text"
                value={firstName}
                onChange={(e) => setFirstName(e.target.value)}
                placeholder="First name"
                required
                className="h-12 rounded-2xl px-3.5 text-[15px] font-medium"
              />
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="last-name" className="pl-0.5 text-[13px] font-bold text-foreground/85">
                Last name
              </Label>
              <Input
                id="last-name"
                type="text"
                value={lastName}
                onChange={(e) => setLastName(e.target.value)}
                placeholder="Last name"
                required
                className="h-12 rounded-2xl px-3.5 text-[15px] font-medium"
              />
            </div>
          </div>

          {createUser.isError && (
            <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-3 text-sm font-medium text-destructive">
              {createUser.error?.toString() || 'Failed to create profile'}
            </div>
          )}

          <Button
            type="submit"
            disabled={createUser.isPending || !canSubmit}
            shape="pill"
            size="lg"
            className="w-full"
          >
            {createUser.isPending ? 'Creating profile…' : 'Create profile'}
          </Button>
        </form>
      </div>
    </div>
  )
}
