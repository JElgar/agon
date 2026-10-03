import { useLayoutEffect, useRef } from 'react'
import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Home, Plus, User } from 'lucide-react'
import { cn } from '@/lib/utils'

/** The floating "log a match" button pokes up above the nav row itself
 *  (negative margin, so it's a circle overlapping the row's top edge) — a
 *  couple of pixels of clearance on top of its own measured top keeps a
 *  pinned bar's bottom edge from clipping into its ring. */
const FLOATING_BUTTON_CLEARANCE_PX = 6

/**
 * The mobile navigation: a fixed bottom tab bar replacing the sidebar sheet
 * on small screens, where a slide-in panel is an awkward reach. Three stops —
 * Feed, a floating "log a match" action, and Profile — everything else
 * (search, notifications, teams, sign out) lives behind the icons in the
 * mobile top bar or on the profile page itself. Hidden at the `md` breakpoint,
 * where the fixed sidebar takes over.
 *
 * Publishes its own full visual height — the icon/label row, the safe-area
 * inset, and the floating button's upward overflow — as `--mobile-nav-height`
 * on the root element, so pinned bars like `MatchActionBar` can sit flush
 * above it (clearing the floating button too) instead of guessing a pixel
 * value that drifts out of sync whenever this nav's content, the floating
 * button, or the device's safe area changes.
 */
export function MobileBottomNav() {
  const location = useLocation()
  const navigate = useNavigate()
  const navRef = useRef<HTMLElement>(null)
  const logMatchRef = useRef<HTMLButtonElement>(null)

  const isActive = (to: string) => location.pathname === to

  useLayoutEffect(() => {
    const nav = navRef.current
    const logMatchButton = logMatchRef.current
    if (!nav || !logMatchButton) return
    const setHeight = () => {
      const visualTop = Math.min(nav.getBoundingClientRect().top, logMatchButton.getBoundingClientRect().top)
      const height = window.innerHeight - visualTop + FLOATING_BUTTON_CLEARANCE_PX
      document.documentElement.style.setProperty('--mobile-nav-height', `${height}px`)
    }
    setHeight()
    const observer = new ResizeObserver(setHeight)
    observer.observe(nav)
    observer.observe(logMatchButton)
    return () => observer.disconnect()
  }, [])

  return (
    <nav
      ref={navRef}
      className="fixed inset-x-0 bottom-0 z-20 border-t bg-card/95 backdrop-blur supports-[backdrop-filter]:bg-card/80 md:hidden"
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
          ref={logMatchRef}
          type="button"
          onClick={() => navigate('/matches/new')}
          aria-label="Log a match"
          className="-mt-6 flex size-14 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg ring-4 ring-background transition-transform active:scale-95"
        >
          <Plus className="size-6" />
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
