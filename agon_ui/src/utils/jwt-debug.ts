// Utility to decode JWT token for debugging (without verification)
export function decodeJwtPayload(token: string) {
  try {
    const parts = token.split('.')
    if (parts.length !== 3) {
      throw new Error('Invalid JWT format')
    }
    
    const payload = parts[1]
    const decoded = atob(payload.replace(/-/g, '+').replace(/_/g, '/'))
    return JSON.parse(decoded)
  } catch (error) {
    console.error('Failed to decode JWT:', error)
    return null
  }
}

// Debug function to log JWT info
export function debugJwt() {
  import('../lib/supabase').then(({ supabase }) => {
    supabase.auth.getSession().then(({ data: { session } }) => {
      if (session?.access_token) {
        const payload = decodeJwtPayload(session.access_token)
        console.log('JWT Payload:', payload)
        console.log('JWT Token (first 50 chars):', session.access_token.substring(0, 50) + '...')
      } else {
        console.log('No session found')
      }
    })
  })
}