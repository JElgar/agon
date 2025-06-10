// Utility to debug user metadata from Supabase
export function debugUserMetadata(user: any) {
  if (!user) {
    console.log('No user found')
    return
  }

  console.log('=== USER DEBUG INFO ===')
  console.log('User object:', user)
  console.log('Email:', user.email)
  console.log('User metadata:', user.user_metadata)
  console.log('App metadata:', user.app_metadata)
  console.log('Identities:', user.identities)
  
  if (user.user_metadata) {
    console.log('=== METADATA FIELDS ===')
    Object.keys(user.user_metadata).forEach(key => {
      console.log(`${key}:`, user.user_metadata[key])
    })
  }
  
  if (user.identities?.[0]) {
    console.log('=== IDENTITY DATA ===')
    console.log('Provider:', user.identities[0].provider)
    console.log('Identity data:', user.identities[0].identity_data)
  }
}