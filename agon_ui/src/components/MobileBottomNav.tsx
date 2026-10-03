import { useLayoutEffect, useRef } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Home, Plus, User } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * The mobile navigation: a fixed bottom tab bar replacing the sidebar sheet
 * on small screens, where a slide-in panel is an awkward reach. Three stops —
 * Feed, Add (logging a match), and Profile — everything else (search,
 * notifications, teams, sign out) lives behind the icons in the mobile top
 * bar or on the profile page itself. Hidden at the `md` breakpoint, where
 * the fixed sidebar takes over.
 *
 * Publishes its own height as `--mobile-nav-height` on the root element, so
 * pinned bars like `MatchActionBar` can sit flush above it instead of
 * guessing a pixel value that drifts out of sync whenever this nav's
 * content or the device's safe area changes.
 */
export function MobileBottomNav() {
  const location = useLocation()
  const navigate = useNavigate()
  const navRef = useRef<HTMLElement>(null)

  const isActive = (to: string) => location.pathname === to

  useLayoutEffect(() => {
    const nav = navRef.current
    if (!nav) return
    const setHeight = () => {
      const height = window.innerHeight - nav.getBoundingClientRect().top
      document.documentElement.style.setProperty('--mobile-nav-height', `${height}px`)
    }
    setHeight()
    const observer = new ResizeObserver(setHeight)
    observer.observe(nav)
    return () => observer.disconnect()
  }, [])

  return (
    <nav
      ref={navRef}
      className="fixed inset-x-0 bottom-0 z-20 bg-card/95 shadow-[0_-1px_8px_rgba(0,0,0,0.06)] backdrop-blur supports-[backdrop-filter]:bg-card/80 md:hidden"
    >
      <div
        className="mx-auto flex max-w-xl items-center justify-around px-6"
        style={{ paddingBottom: 'env(safe-area-inset-bottom)' }}
      >
        <Link
          to="/feed"
          className={cn(
            'flex flex-1 flex-col items-center gap-0.5 py-2 text-[11px] font-medium',
            isActive('/feed') ? 'text-primary' : 'text-muted-foreground',
          )}
        >
          <Home className="size-5" />
          Feed
        </Link>

        <button
          type="button"
          onClick={() => navigate('/matches/new')}
          className="flex flex-1 flex-col items-center gap-0.5 py-2 text-[11px] font-medium text-muted-foreground"
        >
          <Plus className="size-5" />
          Add
        </button>

        <Link
          to="/profile"
          className={cn(
            'flex flex-1 flex-col items-center gap-0.5 py-2 text-[11px] font-medium',
            isActive('/profile') ? 'text-primary' : 'text-muted-foreground',
          )}
        >
          <User className="size-5" />
          Profile
        </Link>
      </div>
    </nav>
  )
}
