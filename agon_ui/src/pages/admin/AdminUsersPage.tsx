import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Search, ShieldCheck } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

type AdminUser = components['schemas']['AdminUser']

/** Debounce for the search input, same as `UserSearchPage`. */
const DEBOUNCE_MS = 300

/**
 * Admin panel: view, search, and edit user accounts (`GET`/`PATCH
 * /admin/users...`). First resource in the panel — matches/series/teams get
 * their own pages here later, each following the same list + edit-dialog
 * shape. Deletion isn't offered yet; see `UpdateAdminUserInput`'s doc
 * comment on the backend for why.
 */
export function AdminUsersPage() {
  const [term, setTerm] = useState('')
  const [debounced, setDebounced] = useState('')
  const [cursor, setCursor] = useState<string | undefined>(undefined)
  const [editing, setEditing] = useState<AdminUser | null>(null)

  useEffect(() => {
    const id = setTimeout(() => {
      setDebounced(term.trim())
      setCursor(undefined)
    }, DEBOUNCE_MS)
    return () => clearTimeout(id)
  }, [term])

  const query = useQuery({
    queryKey: ['admin-users', debounced, cursor],
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/admin/users', {
        params: { query: { q: debounced || undefined, cursor } },
      })
      if (error || !data) throw new Error('Failed to list users')
      return data
    },
  })

  const users = query.data?.items ?? []

  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <div>
        <h1 className="font-display text-xl font-extrabold md:text-3xl">
          Users
        </h1>
        <p className="text-sm text-muted-foreground">
          View, search and edit user accounts.
        </p>
      </div>

      <div className="relative max-w-sm">
        <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          value={term}
          onChange={(e) => setTerm(e.target.value)}
          placeholder="Search by name or email…"
          className="pl-9"
        />
      </div>

      {query.isError && (
        <p className="py-8 text-center text-sm text-muted-foreground">
          Couldn't load users.
        </p>
      )}

      {!query.isError && (
        <div className="overflow-hidden rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Email</TableHead>
                <TableHead>Admin</TableHead>
                <TableHead>Followers</TableHead>
                <TableHead>Joined</TableHead>
                <TableHead />
              </TableRow>
            </TableHeader>
            <TableBody>
              {query.isLoading &&
                Array.from({ length: 5 }).map((_, i) => (
                  <TableRow key={i}>
                    <TableCell colSpan={6}>
                      <div className="h-4 w-full animate-pulse rounded bg-muted" />
                    </TableCell>
                  </TableRow>
                ))}
              {!query.isLoading && users.length === 0 && (
                <TableRow>
                  <TableCell
                    colSpan={6}
                    className="py-8 text-center text-muted-foreground"
                  >
                    No users found.
                  </TableCell>
                </TableRow>
              )}
              {users.map((user) => (
                <TableRow key={user.id}>
                  <TableCell className="font-medium">{user.name}</TableCell>
                  <TableCell className="text-muted-foreground">
                    {user.email}
                  </TableCell>
                  <TableCell>
                    {user.is_admin && (
                      <ShieldCheck className="size-4 text-primary" />
                    )}
                  </TableCell>
                  <TableCell>{user.follower_count}</TableCell>
                  <TableCell className="text-muted-foreground">
                    {new Date(user.created_at).toLocaleDateString()}
                  </TableCell>
                  <TableCell>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setEditing(user)}
                    >
                      Edit
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <div className="flex items-center justify-between">
        <Button
          variant="outline"
          size="sm"
          disabled={!cursor}
          onClick={() => setCursor(undefined)}
        >
          Reset to first page
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={!query.data?.next_cursor}
          onClick={() => setCursor(query.data?.next_cursor)}
        >
          Next page
        </Button>
      </div>

      <EditUserDialog user={editing} onClose={() => setEditing(null)} />
    </div>
  )
}

function EditUserDialog({
  user,
  onClose,
}: {
  user: AdminUser | null
  onClose: () => void
}) {
  const queryClient = useQueryClient()
  const [name, setName] = useState('')
  const [isAdmin, setIsAdmin] = useState(false)

  useEffect(() => {
    if (user) {
      setName(user.name)
      setIsAdmin(user.is_admin)
    }
  }, [user])

  const mutation = useMutation({
    mutationFn: async () => {
      if (!user) return
      const { error } = await fetchClient.PATCH('/admin/users/{user_id}', {
        params: { path: { user_id: user.id } },
        body: { name, is_admin: isAdmin },
      })
      if (error) throw new Error('Failed to update user')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['admin-users'] })
      onClose()
    },
  })

  return (
    <Dialog open={!!user} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit user</DialogTitle>
        </DialogHeader>

        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="admin-user-name">Name</Label>
            <Input
              id="admin-user-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div className="flex items-center justify-between rounded-lg border p-3">
            <div className="flex flex-col gap-0.5">
              <Label htmlFor="admin-user-is-admin">Admin access</Label>
              <span className="text-xs text-muted-foreground">
                Grants access to this admin panel.
              </span>
            </div>
            <Switch
              id="admin-user-is-admin"
              checked={isAdmin}
              onCheckedChange={setIsAdmin}
            />
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => mutation.mutate()} disabled={mutation.isPending}>
            {mutation.isPending ? 'Saving…' : 'Save'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
