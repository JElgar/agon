import { Link } from 'react-router-dom'
import { Logo } from '@/components/agon/Logo'

/**
 * Shared chrome for `/privacy` and `/terms`. These two pages are mounted
 * outside `AuthProvider`/`AppShell` entirely (see `App.tsx`) so they render
 * for signed-out visitors too — Google OAuth branding review and the Garmin
 * Connect IQ store listing both need a reachable, no-login URL. That means
 * this layout can't assume any signed-in chrome (sidebar, bottom nav, theme
 * context) is present, so it's deliberately standalone.
 */
export function LegalPageLayout({
  title,
  updated,
  children,
}: {
  title: string
  updated: string
  children: React.ReactNode
}) {
  return (
    <div className="min-h-screen bg-background">
      <header className="border-b">
        <div className="mx-auto flex max-w-2xl items-center justify-between px-4 py-4">
          <Link to="/">
            <Logo />
          </Link>
          <nav className="flex gap-4 text-sm font-medium text-muted-foreground">
            <Link to="/privacy" className="hover:text-foreground">
              Privacy Policy
            </Link>
            <Link to="/terms" className="hover:text-foreground">
              Terms of Service
            </Link>
          </nav>
        </div>
      </header>
      <main className="mx-auto max-w-2xl px-4 py-10">
        <h1 className="text-3xl font-bold tracking-tight text-foreground">{title}</h1>
        <p className="mt-1 text-sm text-muted-foreground">Last updated: {updated}</p>
        <div className="legal-prose mt-8 space-y-6 text-[15px] leading-relaxed text-foreground/90">
          {children}
        </div>
      </main>
    </div>
  )
}

/** A numbered/plain section with a heading, for consistent spacing. */
export function LegalSection({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="space-y-2">
      <h2 className="text-lg font-semibold text-foreground">{title}</h2>
      <div className="space-y-3">{children}</div>
    </section>
  )
}

/** Visually flags a placeholder value (legal name, contact email, governing
 *  law, ...) that was assumed rather than confirmed — see the PR description
 *  for the full list these need checking against. */
export function Placeholder({ children }: { children: React.ReactNode }) {
  return (
    <mark className="rounded bg-amber-200/60 px-1 py-0.5 font-medium text-amber-950 dark:bg-amber-500/20 dark:text-amber-200">
      {children}
    </mark>
  )
}
