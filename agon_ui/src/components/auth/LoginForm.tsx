import { useState } from 'react'
import { supabase } from '@/lib/supabase'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'

export function LoginForm() {
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [message, setMessage] = useState('')

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setMessage('')

    try {
      const { error } = await supabase.auth.signInWithPassword({
        email,
        password,
      })

      if (error) {
        setMessage(error.message)
      }
    } catch (err) {
      console.error("Sign in failed", err);
      setMessage('Authentication service not configured. Please add your Supabase credentials.')
    }

    setLoading(false)
  }

  const handleSignUp = async (e: React.FormEvent) => {
    e.preventDefault()
    setLoading(true)
    setMessage('')

    try {
      const { error } = await supabase.auth.signUp({
        email,
        password,
        options: {
          // Without this, Supabase sends the confirmation link to the
          // project's default Site URL (effectively "/"), so anyone who
          // signed up from an invite link (e.g. `/invite/:token`) lands on
          // the home page instead of back on the invite once they confirm —
          // the pending-invite localStorage fallback (see pendingInvite.ts)
          // only recovers this if the confirmation is opened in the same
          // browser/origin. Preserving the exact URL here fixes the common
          // case directly, same as the Google OAuth button's `redirectTo`.
          emailRedirectTo: window.location.href,
        },
      })

      if (error) {
        setMessage(error.message)
      } else {
        setMessage('Check your email for the confirmation link!')
      }
    } catch (err) {
      console.error("Sign up failed", err);
      setMessage('Authentication service not configured. Please add your Supabase credentials.')
    }

    setLoading(false)
  }

  const handleGoogleLogin = async () => {
    setLoading(true)
    setMessage('')

    try {
      const { error } = await supabase.auth.signInWithOAuth({
        provider: 'google',
        options: {
          // Preserve the exact page (e.g. `/invite/:token`) rather than just
          // the origin, so Google sign-up/sign-in lands the visitor back
          // where they started instead of the home page — same fix as the
          // email confirmation flow above, and for the same reason: the
          // pending-invite localStorage fallback only recovers this when the
          // OAuth round trip lands back in the same browser/origin.
          redirectTo: window.location.href,
        },
      })

      if (error) {
        setMessage(error.message)
        setLoading(false)
      }
      // Don't set loading to false here as we're redirecting
    } catch (err) {
      console.error("Oauth sign in failed", err);
      setMessage('Authentication service not configured. Please add your Supabase credentials.')
      setLoading(false)
    }
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
        {/* Google Login Button */}
        <Button
          type="button"
          onClick={handleGoogleLogin}
          disabled={loading}
          variant="outline"
          shape="pill"
          size="lg"
          className="w-full"
        >
          <svg className="size-5" viewBox="0 0 24 24">
            <path
              fill="#4285F4"
              d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"
            />
            <path
              fill="#34A853"
              d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
            />
            <path
              fill="#FBBC05"
              d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"
            />
            <path
              fill="#EA4335"
              d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"
            />
          </svg>
          {loading ? 'Signing in...' : 'Continue with Google'}
        </Button>

        {/* Divider */}
        <div className="flex items-center gap-3 py-0.5">
          <span className="h-px flex-1 bg-border" />
          <span className="text-xs font-bold tracking-wide uppercase text-muted-foreground">
            Or continue with email
          </span>
          <span className="h-px flex-1 bg-border" />
        </div>

        {/* Email/Password Form */}
        <form className="space-y-3.5">
          <div className="space-y-1.5">
            <Label htmlFor="email" className="pl-0.5 text-[13px] font-bold text-foreground/85">
              Email
            </Label>
            <Input
              id="email"
              type="email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder="you@example.com"
              required
              className="h-12 rounded-2xl px-3.5 text-[15px] font-medium"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="password" className="pl-0.5 text-[13px] font-bold text-foreground/85">
              Password
            </Label>
            <Input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••••"
              required
              className="h-12 rounded-2xl px-3.5 text-[15px] font-medium"
            />
          </div>

          {message && (
            <div className="text-sm text-destructive">{message}</div>
          )}

          <div className="flex gap-2.5 pt-0.5">
            <Button
              type="button"
              onClick={handleLogin}
              disabled={loading}
              shape="pill"
              size="lg"
              className="flex-1"
            >
              {loading ? 'Loading...' : 'Sign in'}
            </Button>

            <Button
              type="button"
              onClick={handleSignUp}
              disabled={loading}
              variant="outline"
              shape="pill"
              size="lg"
              className="flex-1"
            >
              {loading ? 'Loading...' : 'Create account'}
            </Button>
          </div>
        </form>

        <p className="pt-2 text-center text-[13px] text-muted-foreground">
          By continuing you agree to Agon&apos;s Terms and Privacy Policy.
        </p>
      </div>
    </div>
  )
}
