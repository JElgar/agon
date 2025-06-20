import { useState, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useCreateUser } from '@/hooks/useApi'
import { useAuth } from '@/hooks/useAuth'

interface CreateProfileFormProps {
  email: string
  onProfileCreated: () => void
}

export function CreateProfileForm({ email, onProfileCreated }: CreateProfileFormProps) {
  const { user } = useAuth()
  const [firstName, setFirstName] = useState('')
  const [lastName, setLastName] = useState('')
  const [username, setUsername] = useState('')
  const [isCreated, setIsCreated] = useState(false)
  const { loading, error, createUser } = useCreateUser()

  // Pre-populate form with Google profile info
  useEffect(() => {
    if (user) {
      const metadata = user.user_metadata
      const identityData = user.identities?.[0]?.identity_data
      
      // Try to get name from multiple sources
      const profileData = { ...metadata, ...identityData }
      
      // Try different possible field names from Google OAuth
      const googleFirstName = 
        profileData.given_name || 
        profileData.first_name || 
        profileData.name?.split(' ')[0] || 
        ''
        
      const googleLastName = 
        profileData.family_name || 
        profileData.last_name || 
        profileData.name?.split(' ').slice(1).join(' ') || 
        ''
      
      console.log('Extracted names - First:', googleFirstName, 'Last:', googleLastName)
      
      if (googleFirstName) setFirstName(googleFirstName)
      if (googleLastName) setLastName(googleLastName)
    }
  }, [user])

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    
    if (!firstName.trim() || !lastName.trim() || !username.trim()) {
      return
    }

    const result = await createUser({
      email,
      first_name: firstName.trim(),
      last_name: lastName.trim(),
      username: username.trim(),
    })

    if (result) {
      console.log('Profile created successfully:', result)
      setIsCreated(true)
      
      // Small delay to show success message, then redirect
      setTimeout(() => {
        onProfileCreated()
      }, 1000)
    }
  }

  return (
    <div className="w-full max-w-md mx-auto p-6">
      <div className="text-center mb-6">
        {/* Show Google profile picture or initials */}
        {(() => {
          const metadata = user?.user_metadata
          const identityData = user?.identities?.[0]?.identity_data
          const profileData = { ...metadata, ...identityData }
          
          // Try different possible field names for avatar
          const avatarUrl = 
            profileData?.avatar_url || 
            profileData?.picture ||
            null
          
          // Get name for initials fallback
          const name = profileData?.name || profileData?.full_name || 'User'
          const initials = name.split(' ').map((n: string) => n[0]).join('').toUpperCase().slice(0, 2)
          
          console.log('Avatar URL found:', avatarUrl)
          console.log('User initials:', initials)
          
          return (
            <div className="mb-4">
              {avatarUrl ? (
                <img 
                  src={avatarUrl} 
                  alt="Profile" 
                  className="w-20 h-20 rounded-full mx-auto border-2 border-gray-200 object-cover"
                  onError={(e) => {
                    console.log('Image failed to load:', avatarUrl)
                    // Replace with initials fallback
                    const target = e.currentTarget
                    const parent = target.parentElement
                    if (parent) {
                      parent.innerHTML = `
                        <div class="w-20 h-20 rounded-full mx-auto border-2 border-gray-200 bg-gray-100 flex items-center justify-center">
                          <span class="text-xl font-semibold text-gray-600">${initials}</span>
                        </div>
                      `
                    }
                  }}
                  onLoad={() => console.log('Image loaded successfully:', avatarUrl)}
                />
              ) : (
                <div className="w-20 h-20 rounded-full mx-auto border-2 border-gray-200 bg-gray-100 flex items-center justify-center">
                  <span className="text-xl font-semibold text-gray-600">{initials}</span>
                </div>
              )}
            </div>
          )
        })()}
        
        <h2 className="text-2xl font-bold mb-2">Complete Your Profile</h2>
        <p className="text-gray-600">
          {user?.user_metadata?.name 
            ? `Welcome ${user.user_metadata.name}! Please review and confirm your profile information.`
            : 'Welcome! Please provide your name to complete your profile.'
          }
        </p>
        
        {/* Show note if info was pre-filled */}
        {user?.user_metadata?.name && (
          <p className="text-sm text-blue-600 mt-2">
            ✓ Information pre-filled from your Google account
          </p>
        )}
      </div>

      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <Label htmlFor="email">
            Email
          </Label>
          <Input
            id="email"
            type="email"
            value={email}
            disabled
            className="bg-muted text-muted-foreground"
          />
        </div>

        <div>
          <Label htmlFor="firstName">
            First Name
          </Label>
          <Input
            id="firstName"
            type="text"
            value={firstName}
            onChange={(e) => setFirstName(e.target.value)}
            placeholder="Enter your first name"
            required
          />
        </div>

        <div>
          <Label htmlFor="lastName">
            Last Name
          </Label>
          <Input
            id="lastName"
            type="text"
            value={lastName}
            onChange={(e) => setLastName(e.target.value)}
            placeholder="Enter your last name"
            required
          />
        </div>

        <div>
          <Label htmlFor="username">
            Username
          </Label>
          <Input
            id="username"
            type="text"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="Enter your username"
            required
          />
        </div>

        {error && (
          <div className="p-4 bg-red-50 border border-red-200 rounded-md">
            <p className="text-red-800 text-sm">{error}</p>
          </div>
        )}

        {isCreated && (
          <div className="p-4 bg-green-50 border border-green-200 rounded-md">
            <p className="text-green-800 text-sm">✓ Profile created successfully! Redirecting...</p>
          </div>
        )}

        <Button
          type="submit"
          disabled={loading || isCreated || !firstName.trim() || !lastName.trim() || !username.trim()}
          className="w-full"
        >
          {isCreated ? 'Profile Created!' : loading ? 'Creating Profile...' : 'Create Profile'}
        </Button>
      </form>
    </div>
  )
}
