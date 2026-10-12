import { Link, useLocation, useNavigate } from 'react-router-dom'
import { Bell, Home, LogOut, Plus, Search, User } from 'lucide-react'
import { cn } from '@/lib/utils'
import { Avatar } from '@/components/agon/Avatar'
import { Logo } from '@/components/agon/Logo'
import { ThemeToggle } from '@/components/ThemeToggle'
import { Button } from '@/components/ui/button'
import { useCurrentUserProfile } from '@/hooks/useCurrentUserProfile'
import { totalMatches } from '@/lib/stats'

const NAV_ITEMS = [
  { to: '/feed', label: 'Home', icon: Home },
  { to: '/search', label: 'Search', icon: Search },
  { to: '/notifications', label: 'Notifications', icon: Bell },
  { to: '/profile', label: 'Profile', icon: User },
]

/** Fixed width of `DesktopNav`; the shell (`App.tsx`, `xl:pl-[248px]`)
 *  offsets its content by this much at the `xl` breakpoint, where this
 *  sidebar replaces the tablet/mobile nav entirely. */
const DESKTOP_NAV_WIDTH = 248

/**
 * The desktop nav: a fixed full-height sidebar (≥1280px), matching the
 * "DesktopHome" board — wordmark, four nav destinations, a "New match"
 * action, and a footer chip for the signed-in user. Replaces the
 * mobile bottom bar and tablet floating nav at this breakpoint.
 */
export function DesktopNav({
  unread,
  onSignOut,
}: {
  unread: number | undefined
  onSignOut: () => void
}) {
  const location = useLocation()
  const navigate = useNavigate()
  const profile = useCurrentUserProfile()

  const isActive = (to: string) =>
    location.pathname === to || (to !== '/' && location.pathname.startsWith(to))

  return (
    <aside
      style={{ width: DESKTOP_NAV_WIDTH }}
      className="fixed inset-y-0 left-0 z-20 hidden shrink-0 flex-col gap-7 border-r bg-card p-5 xl:flex"
    >
      <Logo className="pl-3.5 text-3xl" />

      <nav className="flex flex-col gap-1">
        {NAV_ITEMS.map((item) => {
          const active = isActive(item.to)
          const showBadge = item.to === '/notifications' && !!unread && unread > 0
          return (
            <Link
              key={item.to}
              to={item.to}
              className={cn(
                'flex h-12 items-center gap-3.5 rounded-2xl px-3.5 text-base font-semibold',
                active
                  ? 'bg-accent text-accent-foreground'
                  : 'text-foreground/80 hover:bg-muted',
              )}
            >
              <item.icon className="size-[22px]" />
              {item.label}
              {showBadge && (
                <span className="ml-auto size-2 shrink-0 rounded-full bg-destructive" />
              )}
            </Link>
          )
        })}
      </nav>

      <Button
        className="h-[52px] gap-2 rounded-2xl bg-foreground text-base font-bold text-background hover:bg-foreground/90"
        onClick={() => navigate('/matches/new')}
      >
        <Plus className="size-[18px]" />
        New match
      </Button>

      <div className="mt-auto flex flex-col gap-2">
        <div className="flex items-center gap-3 rounded-2xl border p-3">
          <Avatar
            name={profile?.name ?? ''}
            imageUrl={profile?.profile_image?.image_url}
            size="lg"
          />
          <div className="flex min-w-0 flex-col">
            <span className="truncate text-[15px] font-bold">{profile?.name}</span>
            <span className="text-[13px] text-muted-foreground">
              {profile ? `${totalMatches(profile.stats)} matches` : ' '}
            </span>
          </div>
        </div>
        <div className="flex items-center justify-between px-1">
          <ThemeToggle />
          <Button
            variant="ghost"
            size="icon"
            className="rounded-full"
            onClick={onSignOut}
            aria-label="Sign out"
          >
            <LogOut className="size-4" />
          </Button>
        </div>
      </div>
    </aside>
  )
}
