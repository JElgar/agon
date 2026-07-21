import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Home, Plus, User } from 'lucide-react'
import { cn } from '@/lib/utils'

/**
 * The mobile navigation: a fixed bottom tab bar replacing the sidebar sheet
 * on small screens, where a slide-in panel is an awkward reach. Three stops —
 * Feed, a floating "log a match" action, and Profile — everything else
 * (search, notifications, teams, sign out) lives behind the icons in the
 * mobile top bar or on the profile page itself. Hidden at the `md` breakpoint,
 * where the fixed sidebar takes over.
 */
export function MobileBottomNav() {
  const location = useLocation()
  const navigate = useNavigate()

  const isActive = (to: string) => location.pathname === to

  return (
    <nav className="fixed inset-x-0 bottom-0 z-20 border-t bg-card/95 backdrop-blur supports-[backdrop-filter]:bg-card/80 md:hidden">
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
