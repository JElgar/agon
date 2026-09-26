import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Bell, Home, Search, User } from 'lucide-react'
import { cn } from '@/lib/utils'
import { Avatar } from '@/components/agon/Avatar'
import { Logo } from '@/components/agon/Logo'
import { Button } from '@/components/ui/button'
import { useCurrentUserProfile } from '@/hooks/useCurrentUserProfile'

/**
 * Top bar for the tablet breakpoint (`md`–`xl`, ~768–1279px), matching the
 * "TabletHome" board: wordmark plus search/notifications/profile-avatar
 * shortcuts. Below `md` the mobile top bar (in `App.tsx`) takes over; at
 * `xl` the sidebar (`DesktopNav`) carries the full nav instead.
 */
export function TabletHeader({ unread }: { unread: number | undefined }) {
  const profile = useCurrentUserProfile()

  return (
    <header className="hidden items-center justify-between px-8 pb-2.5 pt-4.5 md:flex xl:hidden">
      <Logo className="text-3xl" />
      <div className="flex items-center gap-1.5">
        <Button variant="ghost" size="icon" className="rounded-full" asChild>
          <Link to="/search" aria-label="Find people">
            <Search className="size-[22px]" />
          </Link>
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="relative rounded-full"
          asChild
        >
          <Link
            to="/notifications"
            aria-label={unread ? `Notifications, ${unread} new` : 'Notifications'}
          >
            <Bell className="size-[22px]" />
            {!!unread && unread > 0 && (
              <span className="absolute right-[11px] top-[10px] size-2 rounded-full border-2 border-background bg-destructive" />
            )}
          </Link>
        </Button>
        <Link to="/profile" aria-label="Your profile" className="flex size-11 items-center justify-center">
          <Avatar
            name={profile?.name ?? ''}
            imageUrl={profile?.profile_image?.image_url}
            size="lg"
            className="size-9"
          />
        </Link>
      </div>
    </header>
  )
}

const FLOATING_NAV_ITEMS = [
  { to: '/feed', label: 'Home', icon: Home },
  { to: '/profile', label: 'You', icon: User },
]

/**
 * Floating capsule nav pinned to the bottom-center at the tablet breakpoint,
 * matching the "TabletHome" board — home / new-match / profile, unlike the
 * full-width bar mobile uses below `md`.
 */
export function TabletFloatingNav() {
  const location = useLocation()
  const navigate = useNavigate()

  const isActive = (to: string) => location.pathname === to

  return (
    <nav className="fixed inset-x-0 bottom-6 z-20 hidden justify-center md:flex xl:hidden">
      <div className="flex items-center gap-2 rounded-full border bg-card p-2 shadow-lg">
        {FLOATING_NAV_ITEMS.slice(0, 1).map((item) => (
          <Link
            key={item.to}
            to={item.to}
            className={cn(
              'flex h-12 items-center gap-2 rounded-full px-4.5 text-[15px] font-bold',
              isActive(item.to)
                ? 'bg-accent text-accent-foreground'
                : 'text-foreground/80',
            )}
          >
            <item.icon className="size-5" />
            {item.label}
          </Link>
        ))}
        <button
          type="button"
          onClick={() => navigate('/matches/new')}
          className="flex h-12 items-center gap-2 rounded-full bg-foreground px-5 text-[15px] font-bold text-background"
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
          >
            <path d="M12 5v14M5 12h14" />
          </svg>
          New match
        </button>
        {FLOATING_NAV_ITEMS.slice(1).map((item) => (
          <Link
            key={item.to}
            to={item.to}
            className={cn(
              'flex h-12 items-center gap-2 rounded-full px-4.5 text-[15px] font-semibold',
              isActive(item.to)
                ? 'bg-accent text-accent-foreground'
                : 'text-foreground/80',
            )}
          >
            <item.icon className="size-5" />
            {item.label}
          </Link>
        ))}
      </div>
    </nav>
  )
}
