import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuth } from '@/hooks/useAuth'
import { LoginForm } from '@/components/auth/LoginForm'
import { getRuntimeEnv } from '@/utils/runtime-env'

export function LoginPage() {
  const { session, loading } = useAuth()
  const navigate = useNavigate()
  const env = getRuntimeEnv()

  // Redirect authenticated users to groups
  useEffect(() => {
    if (!loading && session) {
      navigate('/groups', { replace: true })
    }
  }, [session, loading, navigate])

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div>Loading...</div>
      </div>
    )
  }

  if (session) {
    return null // Will redirect
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background">
      <div className="w-full max-w-md p-6">
        <h1 className="text-3xl font-bold text-center mb-8">Welcome to Agon</h1>
        {!env.VITE_SUPABASE_URL && (
          <div className="mb-6 p-4 bg-yellow-50 border border-yellow-200 rounded-md">
            <p className="text-sm text-yellow-800">
              <strong>Development Mode:</strong> Configure Supabase credentials in your .env file to enable authentication.
            </p>
          </div>
        )}
        {env.VITE_SUPABASE_URL && (
          <div className="mb-6 p-4 bg-yellow-50 border border-yellow-200 rounded-md">
            <p className="text-sm text-yellow-800">
              <strong>Development Mode:</strong> Supabase credentials configured.
            </p>
          </div>
        )}
        <LoginForm />
      </div>
    </div>
  )
}