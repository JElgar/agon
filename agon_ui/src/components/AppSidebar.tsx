import { Link, useLocation, useNavigate } from 'react-router-dom'
import {
  Bell,
  LogOut,
  Plus,
  Search,
  Swords,
  User,
  Users,
} from 'lucide-react'
import { useUnreadNotificationsCount } from '@/hooks/useUnreadCount'
import { Button } from '@/components/ui/button'
import { ThemeToggle } from '@/components/ThemeToggle'
import { Logo } from '@/components/agon/Logo'
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  useSidebar,
} from '@/components/ui/sidebar'

interface NavItem {
  to: string
  label: string
  icon: typeof Bell
}

const NAV_ITEMS: NavItem[] = [
  { to: '/feed', label: 'Feed', icon: Swords },
  { to: '/search', label: 'Find people', icon: Search },
  { to: '/teams', label: 'Teams', icon: Users },
  { to: '/notifications', label: 'Notifications', icon: Bell },
  { to: '/profile', label: 'Profile', icon: User },
]

/**
 * The app's primary navigation. On desktop it's a fixed sidebar; on mobile it
 * collapses behind a hamburger trigger (rendered in the top bar) and slides in
 * as a sheet — the replacement for the old header nav, which overflowed on
 * small screens. Hosts the always-available "Create match" action and a live
 * unread-notifications badge.
 */
export function AppSidebar({
  email,
  onSignOut,
}: {
  email: string
  onSignOut: () => void
}) {
  const location = useLocation()
  const navigate = useNavigate()
  const { isMobile, setOpenMobile } = useSidebar()

  // Live unread-notification count for the nav badge.
  const { data: unread } = useUnreadNotificationsCount()

  /** Close the mobile sheet after navigating, so it doesn't cover the page. */
  const closeOnMobile = () => {
    if (isMobile) setOpenMobile(false)
  }

  const isActive = (to: string) =>
    location.pathname === to ||
    (to !== '/' && location.pathname.startsWith(to))

  return (
    <Sidebar>
      <SidebarHeader className="gap-3 p-4">
        <Logo />
        <Button
          className="w-full justify-start gap-2"
          onClick={() => {
            navigate('/matches/new')
            closeOnMobile()
          }}
        >
          <Plus className="size-4" /> Create match
        </Button>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              {NAV_ITEMS.map((item) => {
                const showBadge =
                  item.to === '/notifications' && !!unread && unread > 0
                return (
                  <SidebarMenuItem key={item.to}>
                    <SidebarMenuButton
                      asChild
                      isActive={isActive(item.to)}
                      tooltip={item.label}
                    >
                      <Link to={item.to} onClick={closeOnMobile}>
                        <item.icon />
                        <span>{item.label}</span>
                      </Link>
                    </SidebarMenuButton>
                    {showBadge && (
                      <SidebarMenuBadge>
                        {unread > 99 ? '99+' : unread}
                      </SidebarMenuBadge>
                    )}
                  </SidebarMenuItem>
                )
              })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter className="gap-3 p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="min-w-0 truncate text-sm text-muted-foreground">
            {email}
          </span>
          <ThemeToggle />
        </div>
        <Button variant="outline" className="w-full gap-2" onClick={onSignOut}>
          <LogOut className="size-4" /> Sign out
        </Button>
      </SidebarFooter>
    </Sidebar>
  )
}
