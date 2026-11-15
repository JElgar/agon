import { useEffect, useRef } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronLeft, Youtube } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'

type YoutubeConnectionStatus = components['schemas']['YoutubeConnectionStatus']

const QUERY_KEY = ['youtube-connection']

/**
 * "Connect YouTube" (Phase 1 of the YouTube video design) — reached from the
 * profile page's Account section, same as `PairedDevicesPage`. Google
 * redirects back here with `?code=...` after consent; this page is what
 * exchanges that code (`POST /youtube/connect`), not the backend directly —
 * `agon_service` never receives Google's redirect itself (see
 * `agon_service/src/youtube.rs`'s module doc comment).
 */
export function YoutubeConnectionPage() {
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [searchParams, setSearchParams] = useSearchParams()
  const code = searchParams.get('code')
  const exchangedCode = useRef<string | null>(null)

  const statusQuery = useQuery({
    queryKey: QUERY_KEY,
    queryFn: async (): Promise<YoutubeConnectionStatus> => {
      const { data, error } = await fetchClient.GET('/youtube/connection')
      if (error || !data) throw new Error('Failed to load YouTube connection status')
      return data
    },
  })

  const connectUrlMutation = useMutation({
    mutationFn: async () => {
      const { data, error, response } = await fetchClient.GET('/youtube/connect-url')
      if (response.status === 503) {
        throw new Error('YouTube connection is not set up on this deployment yet.')
      }
      if (error || !data) throw new Error('Could not start the YouTube connection.')
      return data
    },
    onSuccess: (data) => {
      window.location.href = data.url
    },
  })

  const connectMutation = useMutation({
    mutationFn: async (codeToExchange: string) => {
      const { data, error, response } = await fetchClient.POST('/youtube/connect', {
        body: { code: codeToExchange },
      })
      if (response.status === 503) {
        throw new Error('YouTube connection is not set up on this deployment yet.')
      }
      if (response.status === 400) {
        throw new Error(
          "Google didn't grant offline access — remove Agon's access in your Google " +
            'account settings, then try connecting again.',
        )
      }
      if (error || !data) throw new Error('Could not finish connecting YouTube.')
      return data
    },
    onSuccess: (data) => {
      queryClient.setQueryData(QUERY_KEY, data)
      // Drop `?code=...` from the URL now that it's been used — a stray
      // reload shouldn't try to redeem an already-spent code.
      setSearchParams({}, { replace: true })
    },
  })

  const disconnectMutation = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.DELETE('/youtube/connection')
      if (error) throw new Error('Could not disconnect YouTube.')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: QUERY_KEY })
    },
  })

  useEffect(() => {
    if (code && exchangedCode.current !== code) {
      exchangedCode.current = code
      connectMutation.mutate(code)
    }
    // connectMutation is stable across renders (react-query); only `code`
    // should re-trigger this.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code])

  const connected = connectMutation.data?.connected ?? statusQuery.data?.connected
  const channelTitle = connectMutation.data?.channel_title ?? statusQuery.data?.channel_title

  return (
    <div className="mx-auto flex max-w-md flex-col gap-4">
      <div className="flex items-center gap-2">
        <Button variant="ghost" size="icon" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="text-lg font-semibold">YouTube</h1>
      </div>

      {statusQuery.isLoading && !connectMutation.isPending && (
        <p className="text-sm text-muted-foreground">Loading…</p>
      )}

      {statusQuery.isError && (
        <p className="text-sm text-destructive">Couldn't load your YouTube connection.</p>
      )}

      {connectMutation.isPending && (
        <p className="text-sm text-muted-foreground">Finishing connection…</p>
      )}

      {connectMutation.isError && (
        <p className="text-sm text-destructive">{connectMutation.error.message}</p>
      )}

      {connectUrlMutation.isError && (
        <p className="text-sm text-destructive">{connectUrlMutation.error.message}</p>
      )}

      {!connectMutation.isPending && (statusQuery.data || connectMutation.isSuccess) && (
        <div className="flex flex-col gap-3 rounded-xl border bg-card p-4">
          <div className="flex items-center gap-3">
            <div className="flex size-10 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary">
              <Youtube className="size-5" />
            </div>
            <div className="min-w-0">
              <p className="text-sm font-medium">
                {connected ? 'Connected' : 'Not connected'}
              </p>
              {connected && channelTitle && (
                <p className="text-xs text-muted-foreground">as {channelTitle}</p>
              )}
              {!connected && (
                <p className="text-xs text-muted-foreground">
                  Connect your YouTube channel to upload match recordings to it.
                </p>
              )}
            </div>
          </div>

          {connected ? (
            <Button
              variant="outline"
              onClick={() => disconnectMutation.mutate()}
              disabled={disconnectMutation.isPending}
            >
              {disconnectMutation.isPending ? 'Disconnecting…' : 'Disconnect'}
            </Button>
          ) : (
            <Button
              onClick={() => connectUrlMutation.mutate()}
              disabled={connectUrlMutation.isPending}
            >
              {connectUrlMutation.isPending ? 'Opening Google…' : 'Connect YouTube'}
            </Button>
          )}

          {disconnectMutation.isError && (
            <p className="text-sm text-destructive">Could not disconnect. Try again.</p>
          )}
        </div>
      )}
    </div>
  )
}
