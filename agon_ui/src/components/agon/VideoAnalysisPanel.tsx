import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { supabase } from '@/lib/supabase'
import { sideDisplayName } from '@/lib/members'

type Match = components['schemas']['Match']
type Goal = components['schemas']['FootballGoalEvent']
type Point = [number, number]

interface Preview {
  image: string
  fixture: boolean
  rotation: number
  goal: Point[] | null
  start: number
  seconds: number
  kickoff: number
  pitch: [number, number]
  source: { width: number; height: number; fps: string; rotation_metadata: number }
}

interface Candidate {
  id: number
  video_time: number
  match_time: number
  suggested_side: string | null
  side: string | null
  decision: 'pending' | 'confirmed' | 'dismissed'
}

interface Job {
  stage: string
  percent: number
  error: string | null
  video_id: string
  result: null | {
    candidates: Candidate[]
    sample_frames: number
    sample_fps: number
    goalmouth_frames: number
    other_end: 'unobserved'
  }
}

const fixtureLink = 'https://www.youtube.com/watch?v=7g9XA8HWn8M'

function time(seconds: number) {
  return `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`
}

async function request(matchId: string, path: string, options?: RequestInit) {
  const token = import.meta.env.MODE === 'video-demo'
    ? 'fixture-organiser'
    : (await supabase.auth.getSession()).data.session?.access_token
  if (!token) throw new Error('Sign in to analyse a match video.')
  const response = await fetch(`/video-analysis/${encodeURIComponent(matchId)}${path}`, {
    ...options,
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    cache: 'no-store',
  })
  if (!response.ok) {
    const body = await response.json().catch(() => null)
    throw new Error(typeof body?.detail === 'string' ? body.detail : 'Video analysis could not complete this request.')
  }
  return response
}

export function VideoAnalysisPanel({ match, canImport, onImport }: {
  match: Match
  canImport: boolean
  onImport: (goals: Goal[]) => void
}) {
  const storageKey = `agon:video-job:${match.id}`
  const [link, setLink] = useState(import.meta.env.MODE === 'video-demo' ? fixtureLink : '')
  const [job, setJob] = useState(() => sessionStorage.getItem(storageKey))
  const [preview, setPreview] = useState<Preview | null>(null)
  const [goal, setGoal] = useState<Point[]>([])
  const [checked, setChecked] = useState(false)
  const [bibs, setBibs] = useState<Record<string, string>>({})
  const sides = match.sides.map((side, index) => ({
    id: side.id,
    name: sideDisplayName(side) ?? `Side ${index + 1}`,
    colour: side.colour && /^#[\da-f]{6}$/i.test(side.colour)
      ? side.colour
      : index === 0 ? '#3764e6' : '#e04b50',
  }))

  const load = useMutation({
    mutationFn: async (rotation: number | null) => (await request(match.id, '/preview', {
      method: 'POST', body: JSON.stringify({ youtube: link, rotation }),
    })).json() as Promise<Preview>,
    onSuccess: (value) => {
      setPreview(value)
      setGoal(value.goal ?? [])
      setChecked(false)
    },
  })
  const start = useMutation({
    mutationFn: async () => {
      if (!preview) throw new Error('Load a preview first.')
      const settings = {
        youtube: link, start: preview.start, seconds: preview.seconds, kickoff: preview.kickoff,
        rotation: preview.rotation, goal, upright_checked: checked,
        bibs: Object.fromEntries(sides.map((side) => [side.id, bibs[side.id] ?? side.colour])),
      }
      return (await request(match.id, '/jobs', {
        method: 'POST', body: JSON.stringify(settings),
      })).json() as Promise<{ id: string }>
    },
    onSuccess: ({ id }) => {
      sessionStorage.setItem(storageKey, id)
      setJob(id)
      if (id === job) void status.refetch()
    },
  })
  const status = useQuery({
    queryKey: ['video-analysis', match.id, job],
    enabled: !!job,
    queryFn: async () => (await request(match.id, `/jobs/${job}`)).json() as Promise<Job>,
    refetchInterval: (query) => {
      const stage = query.state.data?.stage
      return stage === 'Ready' || stage === 'Failed' ? false : 650
    },
  })
  const jobInfo = status.data
  const confirmed = jobInfo?.result?.candidates.filter((candidate) => candidate.decision === 'confirmed' && candidate.side) ?? []

  function importDraft() {
    const unique = new Map(confirmed.map((candidate) => [candidate.id, candidate]))
    const goals: Goal[] = [...unique.values()].flatMap((candidate) => candidate.side ? [{
      side_id: candidate.side, minute: Math.floor(candidate.match_time / 60), own_goal: false, penalty: false,
    }] : [])
    onImport(goals)
  }

  return (
    <section className="space-y-3 rounded-xl border bg-card p-4" aria-label="Video analysis">
      <h2 className="font-semibold">Video analysis</h2>
      <form className="space-y-2" onSubmit={(event) => { event.preventDefault(); load.mutate(null) }}>
        <label className="block space-y-1 text-sm">
          <span>YouTube link</span>
          <Input type="url" required value={link} disabled={load.isPending || start.isPending} onChange={(event) => {
            setLink(event.target.value)
            setPreview(null)
            setChecked(false)
            load.reset()
          }} />
        </label>
        <Button type="submit" variant="outline" disabled={load.isPending || start.isPending}>
          {load.isPending ? 'Loading preview…' : 'Check orientation'}
        </Button>
      </form>
      {load.error && <p role="alert" className="text-sm text-destructive">{load.error.message}</p>}
      {preview && (
        <div className="space-y-3">
          {preview.fixture && <p className="text-sm font-medium text-amber-700 dark:text-amber-300">Synthetic fixture footage · 10:00–12:00 on the pilot timeline</p>}
          <p className="text-sm text-muted-foreground">
            {time(preview.start)}–{time(preview.start + preview.seconds)} · {preview.source.width} × {preview.source.height} at {preview.source.fps} fps.
            Pitch estimate {preview.pitch.join(' × ')} m. Opposite end unobserved.
          </p>
          <div className="relative">
            <img className="block w-full rounded-md" src={preview.image} alt="Orientation and goalmouth preview" />
            <svg
              viewBox="0 0 1000 1000" preserveAspectRatio="none"
              className="pointer-events-none absolute inset-0 h-full w-full"
              aria-hidden="true"
            >
              {goal.length >= 2 && <polygon points={goal.map(([x, y]) => `${x * 1000},${y * 1000}`).join(' ')}
                fill="rgba(255, 221, 70, .19)" stroke="#ffdd46" strokeWidth="3" vectorEffect="non-scaling-stroke" />}
              {goal.map(([x, y], index) => <circle key={index} cx={x * 1000} cy={y * 1000} r="9" fill="#ffdd46" />)}
            </svg>
            <button
              type="button"
              className="absolute inset-0 cursor-crosshair rounded-md border-0 bg-transparent"
              disabled={goal.length >= 4 || load.isPending || start.isPending}
              aria-label={`Mark goal corner ${goal.length + 1}`}
              onClick={(event) => {
                if (event.detail === 0) return
                const bounds = event.currentTarget.getBoundingClientRect()
                setGoal([...goal, [(event.clientX - bounds.left) / bounds.width, (event.clientY - bounds.top) / bounds.height]])
              }}
            />
          </div>
          <p className="text-xs text-muted-foreground">Tap both front posts, then the two back corners around the goal. {goal.length}/4 marked.</p>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" disabled={load.isPending || start.isPending}
              onClick={() => load.mutate((preview.rotation + 90) % 360)}>Rotate 90°</Button>
            <Button variant="outline" size="sm" disabled={!goal.length || start.isPending}
              onClick={() => setGoal([])}>Redraw goal</Button>
          </div>
          <label className="flex min-h-11 items-center gap-2 text-sm">
            <input type="checkbox" checked={checked} onChange={(event) => setChecked(event.target.checked)} />
            Frame is upright
          </label>
          <div className="flex flex-wrap gap-4">
            {sides.map((side) => (
              <label className="flex min-h-11 items-center gap-2 text-sm" key={side.id}>
                <input type="color" value={bibs[side.id] ?? side.colour}
                  onChange={(event) => setBibs({ ...bibs, [side.id]: event.target.value })} />
                {side.name} bib
              </label>
            ))}
          </div>
          <Button onClick={() => start.mutate()} disabled={!checked || goal.length !== 4 || start.isPending || load.isPending}>
            Analyse video
          </Button>
          {start.error && <p role="alert" className="text-sm text-destructive">{start.error.message}</p>}
        </div>
      )}
      {jobInfo && jobInfo.stage !== 'Ready' && (
        <div role="status" className="space-y-1 text-sm">
          <p>{jobInfo.stage}{jobInfo.stage !== 'Failed' && ` · ${jobInfo.percent}%`}</p>
          {jobInfo.error
            ? <p className="text-destructive">{jobInfo.error}</p>
            : <progress className="w-full" max={100} value={jobInfo.percent} aria-label="Analysis progress" />}
        </div>
      )}
      {status.error && <p role="alert" className="text-sm text-destructive">{status.error.message}</p>}
      {job && jobInfo?.result && (
        <div className="space-y-3 border-t pt-3">
          <h3 className="text-sm font-semibold">Review candidate goals</h3>
          <p className="text-sm text-muted-foreground">
            {jobInfo.result.sample_frames} frames at {jobInfo.result.sample_fps} fps · {jobInfo.result.goalmouth_frames} source-rate frames.
            Visible end observed. Opposite end {jobInfo.result.other_end}.
          </p>
          {!jobInfo.result.candidates.length && <p className="text-sm">No candidates in this window.</p>}
          {jobInfo.result.candidates.map((candidate) => (
            <CandidateRow key={`${job}/${candidate.id}`} matchId={match.id} job={job}
              candidate={candidate} sides={sides} refresh={() => status.refetch()} />
          ))}
          {confirmed.length > 0 && (
            canImport
              ? <Button onClick={importDraft}>Open {confirmed.length} confirmed in result editor</Button>
              : <p className="text-sm text-muted-foreground">The match already has a result or is live. The reviewed goals remain available here.</p>
          )}
        </div>
      )}
    </section>
  )
}

function CandidateRow({ matchId, job, candidate, sides, refresh }: {
  matchId: string
  job: string
  candidate: Candidate
  sides: { id: string; name: string }[]
  refresh: () => unknown
}) {
  const [side, setSide] = useState(candidate.side ?? '')
  const [clip, setClip] = useState<string>()
  const review = useMutation({
    mutationFn: async (decision: string) => request(matchId, `/jobs/${job}/review/${candidate.id}`, {
      method: 'PUT', body: JSON.stringify({ decision, side: side || null }),
    }),
    onSuccess: () => { refresh() },
  })
  const openClip = useMutation({
    mutationFn: async () => URL.createObjectURL(await (await request(matchId, `/jobs/${job}/clips/${candidate.id}`)).blob()),
    onSuccess: setClip,
  })
  useEffect(() => () => { if (clip) URL.revokeObjectURL(clip) }, [clip])
  const suggested = sides.find((item) => item.id === candidate.suggested_side)?.name ?? 'Unknown'
  return (
    <article className="space-y-2 rounded-lg border bg-muted/20 p-3">
      <p className="text-sm font-medium">Video {time(candidate.video_time)} · match {time(candidate.match_time)}</p>
      <p className="text-xs text-muted-foreground">Suggested side {suggested} · {candidate.decision}</p>
      {clip
        ? <video className="w-full rounded-md" controls playsInline preload="metadata" src={clip} />
        : <Button variant="outline" size="sm" onClick={() => openClip.mutate()} disabled={openClip.isPending}>Inspect footage</Button>}
      <label className="flex flex-col gap-1 text-sm">
        Scoring side
        <select value={side} aria-label={`Scoring side at ${time(candidate.video_time)}`}
          className="min-h-11 rounded-md border bg-background px-2" onChange={(event) => setSide(event.target.value)}>
          <option value="">Unknown</option>
          {sides.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
        </select>
      </label>
      <div className="flex flex-wrap gap-2">
        <Button onClick={() => review.mutate('confirmed')} disabled={!side || review.isPending}>Confirm goal</Button>
        <Button variant="outline" onClick={() => review.mutate('dismissed')} disabled={review.isPending}>Dismiss</Button>
      </div>
      {(review.error || openClip.error) && <p role="alert" className="text-sm text-destructive">{(review.error ?? openClip.error)?.message}</p>}
    </article>
  )
}
