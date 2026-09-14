import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useMutation } from '@tanstack/react-query'
import { Watch } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { clearPendingInvite } from '@/lib/pendingInvite'
import { apiErrorMessage } from '@/lib/api-error'

/**
 * The device-pairing confirmation screen. Reached either by scanning a
 * Garmin watch's QR code (`/pair?code=...` — see `qr::render_pairing_qr`
 * on the server) or by typing in the code shown as that QR's fallback; the
 * login gate routes through sign-in/signup first, same as
 * `AcceptInvitePage`/`JoinMatchPage`, via the same pending-link mechanism
 * (see `App.tsx`'s `usePendingLinkCapture`).
 *
 * There's nothing to preview here (unlike an invite/join link) — the
 * pairing code doesn't exist as a server-side record at all until this
 * page actually confirms it (see `agon_core::dao::device_pairing`'s doc
 * comment), so the only two states are "here's the code, confirm it?" and
 * whatever `POST /devices/pairing-codes/:code/confirm` comes back with. The
 * watch itself is what's polling `POST /devices/pair` in the background —
 * this page never calls that endpoint, it only confirms.
 */
export function PairDevicePage() {
  const [searchParams] = useSearchParams()
  const navigate = useNavigate()
  const codeFromUrl = normalizeCode(searchParams.get('code') ?? '')

  const [code, setCode] = useState(codeFromUrl)
  // No code on the URL (someone navigated here directly, or the QR scan
  // failed and they're using the fallback text) — start on the manual-entry
  // form instead of a confirm screen for a blank code.
  const [editing, setEditing] = useState(!codeFromUrl)

  // We're now on the pairing URL itself; the stashed copy (used to survive
  // login) has done its job.
  useEffect(() => {
    clearPendingInvite()
  }, [])

  const confirm = useMutation({
    mutationFn: async (): Promise<'confirmed' | 'invalid'> => {
      const { error, response } = await fetchClient.POST(
        '/devices/pairing-codes/{code}/confirm',
        { params: { path: { code } } },
      )
      if (response.status === 409) return 'invalid'
      if (error) throw new Error(apiErrorMessage(error, 'Failed to confirm pairing code'))
      return 'confirmed'
    },
  })

  if (editing) {
    return (
      <PairCard>
        <PairIcon />
        <h2 className="mb-1 text-xl font-semibold">Pair a watch</h2>
        <p className="mb-6 text-sm text-muted-foreground">
          Enter the code shown on your watch — or scan its QR code with your
          phone's camera to skip this step.
        </p>
        <form
          className="w-full"
          onSubmit={(e) => {
            e.preventDefault()
            const trimmed = normalizeCode(code)
            if (!trimmed) return
            setCode(trimmed)
            setEditing(false)
            confirm.reset()
          }}
        >
          <Input
            autoFocus
            value={code}
            onChange={(e) => setCode(normalizeCode(e.target.value))}
            placeholder="ABC123"
            className="mb-4 text-center font-mono text-lg tracking-[0.3em]"
            maxLength={8}
          />
          <Button type="submit" className="w-full" disabled={!code.trim()}>
            Continue
          </Button>
        </form>
      </PairCard>
    )
  }

  if (confirm.data === 'confirmed') {
    return (
      <PairCard>
        <PairIcon />
        <h2 className="mb-1 text-xl font-semibold">Watch paired</h2>
        <p className="mb-6 text-sm text-muted-foreground">
          Check your watch — it should move on by itself within a few
          seconds.
        </p>
        <Button className="w-full" onClick={() => navigate('/feed', { replace: true })}>
          Done
        </Button>
      </PairCard>
    )
  }

  return (
    <PairCard>
      <PairIcon />
      <h2 className="mb-1 text-xl font-semibold">Pair this watch?</h2>
      <p className="mb-4 text-sm text-muted-foreground">
        Only confirm if this is the code currently showing on your own watch.
      </p>
      <p className="mb-6 w-full rounded-lg border bg-muted/30 px-4 py-3 text-center font-mono text-2xl font-semibold tracking-[0.3em]">
        {code}
      </p>

      {confirm.data === 'invalid' && (
        <p className="mb-3 text-sm text-destructive">
          This code has already been used or has expired. Check your watch
          for a fresh one.
        </p>
      )}
      {confirm.isError && (
        <p className="mb-3 text-sm text-destructive">Something went wrong. Try again.</p>
      )}

      <Button className="w-full" disabled={confirm.isPending} onClick={() => confirm.mutate()}>
        {confirm.isPending ? 'Confirming…' : 'Confirm pairing'}
      </Button>
      <Button
        variant="ghost"
        className="mt-2 w-full"
        onClick={() => {
          setEditing(true)
          setCode('')
          confirm.reset()
        }}
      >
        Use a different code
      </Button>
    </PairCard>
  )
}

/** Uppercased/trimmed the same way the server normalizes a submitted code
 *  (see `confirm_device_pairing`) — purely cosmetic here, since the server
 *  re-normalizes regardless, but avoids a confusing case-mismatch flash
 *  between what's typed and what's shown. */
function normalizeCode(raw: string): string {
  return raw.trim().toUpperCase()
}

function PairIcon() {
  return (
    <div className="mb-4 flex size-14 items-center justify-center rounded-full bg-primary/10 text-primary">
      <Watch className="size-7" />
    </div>
  )
}

/** Centered card chrome, mirroring `JoinMatchPage`/`AcceptInvitePage`'s own. */
function PairCard({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center rounded-2xl border bg-card p-8 text-center">
      {children}
    </div>
  )
}
