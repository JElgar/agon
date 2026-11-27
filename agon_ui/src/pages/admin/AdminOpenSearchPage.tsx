import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

/**
 * Admin panel: trigger the `agon_worker backfill-opensearch` job and watch it
 * run, instead of a manual `kubectl exec` into the worker pod. `POST`s
 * `/admin/opensearch/backfill` to start it (refused with a running job's
 * status if one is already in flight) and polls `GET` on the same path every
 * few seconds while `status: "running"`, same job the CLI runs — see
 * `agon_services_core::opensearch::backfill`'s doc comment for when to use
 * it (first load, after a schema version bump, or as a repair tool).
 */
export function AdminOpenSearchPage() {
  const queryClient = useQueryClient()
  const [deleteOld, setDeleteOld] = useState(false)

  const status = useQuery({
    queryKey: ['admin-opensearch-backfill'],
    queryFn: async () => {
      const { data, error } = await fetchClient.GET('/admin/opensearch/backfill')
      if (error || !data) throw new Error('Failed to load backfill status')
      return data
    },
    refetchInterval: (query) =>
      query.state.data?.status === 'running' ? 3000 : false,
  })

  const start = useMutation({
    mutationFn: async () => {
      const { data, error, response } = await fetchClient.POST(
        '/admin/opensearch/backfill',
        { body: { delete_old: deleteOld } },
      )
      if (response.status === 409) {
        throw new Error('A backfill is already running')
      }
      if (error || !data) throw new Error('Failed to start backfill')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(['admin-opensearch-backfill'], data)
      queryClient.invalidateQueries({ queryKey: ['admin-opensearch-backfill'] })
    },
  })

  const job = status.data
  const isRunning = job?.status === 'running'

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-4">
      <div>
        <h1 className="font-display text-xl font-extrabold md:text-3xl">
          OpenSearch
        </h1>
        <p className="text-sm text-muted-foreground">
          Rebuild the match and user documents in OpenSearch from DynamoDB.
          Use this after a schema version bump, or any time as a repair tool
          — it's safe to re-run.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Run backfill</CardTitle>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center justify-between rounded-lg border p-3">
            <div className="flex flex-col gap-0.5">
              <Label htmlFor="backfill-delete-old">Delete old index</Label>
              <span className="text-xs text-muted-foreground">
                Remove the superseded index version once the alias swap
                completes. Leave off to keep it around for a rollback.
              </span>
            </div>
            <Switch
              id="backfill-delete-old"
              checked={deleteOld}
              onCheckedChange={setDeleteOld}
              disabled={isRunning}
            />
          </div>

          <div className="flex items-center gap-3">
            <Button
              onClick={() => start.mutate()}
              disabled={isRunning || start.isPending}
            >
              {isRunning ? (
                <>
                  <RefreshCw className="size-4 animate-spin" />
                  Running…
                </>
              ) : (
                'Run backfill'
              )}
            </Button>
            {start.isError && (
              <span className="text-sm text-destructive">
                {start.error instanceof Error
                  ? start.error.message
                  : 'Failed to start backfill'}
              </span>
            )}
          </div>
        </CardContent>
      </Card>

      {job && job.status !== 'idle' && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">
              {isRunning ? 'In progress' : 'Last run'}
            </CardTitle>
          </CardHeader>
          <CardContent className="flex flex-col gap-3">
            <dl className="grid grid-cols-2 gap-y-2 text-sm sm:grid-cols-4">
              <StatusField label="Status" value={job.status} />
              <StatusField
                label="Started"
                value={
                  job.started_at
                    ? new Date(job.started_at).toLocaleTimeString()
                    : '—'
                }
              />
              <StatusField
                label="Match pages"
                value={job.match_pages.toLocaleString()}
              />
              <StatusField
                label="Matches indexed"
                value={job.matches_indexed.toLocaleString()}
              />
              <StatusField
                label="Matches failed"
                value={job.matches_failed.toLocaleString()}
              />
              <StatusField
                label="User pages"
                value={job.user_pages.toLocaleString()}
              />
              <StatusField
                label="Users indexed"
                value={job.users_indexed.toLocaleString()}
              />
              <StatusField
                label="Users failed"
                value={job.users_failed.toLocaleString()}
              />
            </dl>
            {job.error && (
              <p className="text-sm text-destructive">{job.error}</p>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function StatusField({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex flex-col">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="font-medium">{value}</dd>
    </div>
  )
}
