import { Users } from 'lucide-react'
import { NavLink, Outlet } from 'react-router-dom'
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
} from '@/components/ui/sidebar'
import { cn } from '@/lib/utils'

/** Resources the admin panel covers. Just users for now — matches/series/
 *  teams get their own entries here as those pages are built. */
const RESOURCES = [{ to: '/admin/users', label: 'Users', icon: Users }]

/**
 * Shell for every `/admin/...` page: a resource sidebar on the left, the
 * routed page on the right. One sidebar entry per resource the panel
 * covers.
 */
export function AdminLayout() {
  return (
    <SidebarProvider className="min-h-0">
      <Sidebar collapsible="none" className="h-full border-r">
        <SidebarHeader>
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
      <SidebarInset className="px-4 py-6 md:px-8">
        <Outlet />
      </SidebarInset>
    </SidebarProvider>
  )
}
