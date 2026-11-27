import { ArrowLeft, Database, Trophy, Users } from 'lucide-react'
import { Link, NavLink, Outlet } from 'react-router-dom'
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from '@/components/ui/sidebar'
import { cn } from '@/lib/utils'

/** Resources the admin panel covers. Series/teams get their own entries
 *  here as those pages are built. */
const RESOURCES = [
  { to: '/admin/users', label: 'Users', icon: Users },
  { to: '/admin/matches', label: 'Matches', icon: Trophy },
  { to: '/admin/opensearch', label: 'OpenSearch', icon: Database },
]

/**
 * Shell for every `/admin/...` page: its own full-screen layout (rendered
 * outside `AppShell`'s chrome entirely, not nested inside it — see
 * `App.tsx`'s top-level route split), with a resource sidebar on the left
 * and the routed page on the right. One sidebar entry per resource the
 * panel covers.
 *
 * The sidebar collapses to an off-canvas sheet below `md` (the default
 * `collapsible="offcanvas"`, same breakpoint the rest of the app's chrome
 * uses), opened via the `SidebarTrigger` in the mobile-only header bar —
 * admin has no bottom tab bar or top-bar icon of its own to get back in
 * with, so that trigger plus "Back to app" are the only way in/out of the
 * resource list on a small screen.
 */
export function AdminLayout() {
  return (
    <SidebarProvider>
      <Sidebar className="border-r">
        <SidebarHeader className="gap-2">
          <Link
            to="/feed"
            className="flex items-center gap-1.5 px-2 text-sm text-muted-foreground hover:text-foreground"
          >
            <ArrowLeft className="size-4" />
            Back to app
          </Link>
          <span className="font-display px-2 text-lg font-extrabold">Admin</span>
        </SidebarHeader>
        <SidebarContent>
          <SidebarGroup>
            <SidebarGroupLabel>Resources</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {RESOURCES.map(({ to, label, icon: Icon }) => (
                  <SidebarMenuItem key={to}>
                    <NavLink to={to}>
                      {({ isActive }) => (
                        <SidebarMenuButton asChild isActive={isActive}>
                          <span className={cn(isActive && 'font-semibold')}>
                            <Icon className="size-4" />
                            {label}
                          </span>
                        </SidebarMenuButton>
                      )}
                    </NavLink>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>
      </Sidebar>
      <SidebarInset>
        <div className="flex items-center gap-2 border-b px-4 py-3 md:hidden">
          <SidebarTrigger />
          <span className="font-display text-base font-extrabold">Admin</span>
        </div>
        <div className="px-4 py-6 md:px-8">
          <Outlet />
        </div>
      </SidebarInset>
    </SidebarProvider>
  )
}
