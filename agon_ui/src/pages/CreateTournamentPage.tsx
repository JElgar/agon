import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'
import { useNavigate } from 'react-router-dom'
import { ChevronLeft } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { DateTimePicker } from '@/components/ui/date-time-picker'
import { SportPicker } from '@/components/agon/SportPicker'
import { LocationField, type LocationValue } from '@/components/agon/LocationField'
import { CARD, FormSection, Segmented } from '@/components/agon/logmatch/parts'
import { FormatPresets } from '@/components/agon/logmatch/FormatPresets'
import { formatDefaultHint } from '@/lib/logMatch'
import type { MatchFormat } from '@/lib/matchFormat'
import type { MatchType } from '@/lib/sports'

type CreateTournamentInput = components['schemas']['CreateTournamentInput']
type Tournament = components['schemas']['Tournament']
type TournamentFormat = components['schemas']['TournamentFormat']
type TournamentEntryAudience = components['schemas']['TournamentEntryAudience']
type TournamentSeeding = components['schemas']['TournamentSeeding']

const FORMAT_OPTIONS: { value: TournamentFormat; label: string }[] = [
  { value: 'knockout', label: 'Knockout' },
  { value: 'league', label: 'League' },
]

const ENTRY_AUDIENCE_OPTIONS: { value: TournamentEntryAudience; label: string }[] = [
  { value: 'teams', label: 'Teams' },
  { value: 'players', label: 'Players' },
]

const SEEDING_OPTIONS: { value: TournamentSeeding; label: string }[] = [
  { value: 'random', label: 'Random' },
  { value: 'manual', label: 'Manual' },
]

/**
 * `POST /tournaments` — knockout or league, entered by teams or by
 * individual players (who get assigned into fixed squads before it
 * starts — see `TournamentEntriesPage`). A teamless tournament
 * (`entry_audience: "players"`) still forces manual seeding (a squad has
 * no single seed) — auto squad balancing isn't built yet.
 *
 * `match_format` (optional) is applied as every generated match's
 * default, same picker as `LogMatchPage`'s.
 *
 * Scheduling is manual-only for now (2026-11-09): every generated match
 * starts at the one time given here, and the organiser nudges each match's
 * actual time on the existing edit-match page as the tournament is played
 * — auto-filling matches into a recurring or one-day availability window is
 * still being designed, so there's no cadence picker here yet.
 */
export function CreateTournamentPage() {
  const navigate = useNavigate()

  const [sport, setSport] = useState<MatchType | null>(null)
  const [matchFormat, setMatchFormat] = useState<MatchFormat | null>(null)
  const changeSport = (next: MatchType) => {
    setSport(next)
    setMatchFormat(null)
  }
  const [name, setName] = useState('')
  const [format, setFormat] = useState<TournamentFormat>('knockout')
  const [entryAudience, setEntryAudience] = useState<TournamentEntryAudience>('teams')
  const [squadSize, setSquadSize] = useState('')
  const [minTeams, setMinTeams] = useState('4')
  const [maxTeams, setMaxTeams] = useState('')
  const [location, setLocation] = useState<LocationValue | null>(null)
  const [startsAt, setStartsAt] = useState('')
  const [seeding, setSeeding] = useState<TournamentSeeding>('random')

  // A teamless tournament only supports random seeding + manual squad
  // assignment — enforce that in the form rather than letting the user
  // pick an unsupported combination and find out from a 400.
  const isTeamless = entryAudience === 'players'

  const canSave =
    !!sport &&
    name.trim() !== '' &&
    !!startsAt &&
    Number(minTeams) >= 2 &&
    (!isTeamless || squadSize.trim() !== '')

  const mutation = useMutation({
    mutationFn: async (): Promise<Tournament> => {
      const body: CreateTournamentInput = {
        match_type: sport!,
        name: name.trim(),
        description: '',
        format,
        match_format: matchFormat ?? undefined,
        entry_audience: entryAudience,
        team_formation: isTeamless ? 'manual' : undefined,
        squad_size: isTeamless ? Number(squadSize) : undefined,
        min_teams: Number(minTeams),
        max_teams: maxTeams.trim() ? Number(maxTeams) : undefined,
        location: location?.text.trim() ? { ...location, text: location.text.trim() } : undefined,
        first_round_starts_at: new Date(startsAt).toISOString(),
        timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
        seeding: isTeamless ? 'random' : seeding,
      }
      const { data, error } = await fetchClient.POST('/tournaments', { body })
      if (error || !data) throw new Error('Could not create tournament')
      return data
    },
    onSuccess: (tournament) => {
      navigate(`/tournaments/${tournament.id}`, { replace: true })
    },
  })

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-5 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          className="-ml-2"
          onClick={() => navigate(-1)}
          aria-label="Back"
        >
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-2xl">New tournament</h1>
      </div>

      <FormSection title="Sport">
        <SportPicker value={sport} onChange={changeSport} />
      </FormSection>

      {sport && formatDefaultHint(sport) && (
        <FormSection title="Match format" aside="Optional">
          <div className={CARD}>
            <FormatPresets sport={sport} value={matchFormat} onChange={setMatchFormat} />
          </div>
          {matchFormat === null && (
            <p className="px-1 text-xs text-muted-foreground">
              Applied as every match's default — {formatDefaultHint(sport)}
            </p>
          )}
        </FormSection>
      )}

      <FormSection title="Name" htmlFor="tournament-name">
        <Input
          id="tournament-name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="e.g. Riverside Cup"
        />
      </FormSection>

      <FormSection title="Format">
        <Segmented label="Format" options={FORMAT_OPTIONS} value={format} onChange={setFormat} />
      </FormSection>

      <FormSection title="Who enters">
        <Segmented
          label="Who enters"
          options={ENTRY_AUDIENCE_OPTIONS}
          value={entryAudience}
          onChange={setEntryAudience}
        />
        {isTeamless && (
          <div className={CARD}>
            <p className="text-sm text-muted-foreground">
              No pre-existing teams — individuals join, and you assign them into fixed squads
              before the tournament starts.
            </p>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="squad-size">Squad size</Label>
              <Input
                id="squad-size"
                type="number"
                min={1}
                value={squadSize}
                onChange={(e) => setSquadSize(e.target.value)}
                placeholder="e.g. 5"
              />
            </div>
          </div>
        )}
      </FormSection>

      <FormSection title={isTeamless ? 'Squads' : 'Teams'}>
        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="min-teams">Minimum</Label>
            <Input
              id="min-teams"
              type="number"
              min={2}
              value={minTeams}
              onChange={(e) => setMinTeams(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="max-teams">Maximum (optional)</Label>
            <Input
              id="max-teams"
              type="number"
              min={2}
              value={maxTeams}
              onChange={(e) => setMaxTeams(e.target.value)}
            />
          </div>
        </div>
      </FormSection>

      {!isTeamless && (
        <FormSection title="Seeding">
          <Segmented label="Seeding" options={SEEDING_OPTIONS} value={seeding} onChange={setSeeding} />
        </FormSection>
      )}

      <FormSection title="Schedule">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="first-round-starts-at">Starts around</Label>
          <DateTimePicker id="first-round-starts-at" value={startsAt} onChange={setStartsAt} />
          <p className="text-xs text-muted-foreground">
            Every match starts here for now — adjust each one's actual time from its match page
            as you go.
          </p>
        </div>
      </FormSection>

      <FormSection title="Location (optional)">
        <LocationField id="tournament-location" value={location} onChange={setLocation} />
      </FormSection>

      {mutation.isError && (
        <p className="text-sm text-destructive">Could not create this tournament. Try again.</p>
      )}

      <Button
        size="lg"
        className="mt-1"
        onClick={() => mutation.mutate()}
        disabled={mutation.isPending || !canSave}
      >
        {mutation.isPending ? 'Creating…' : 'Create tournament'}
      </Button>
    </div>
  )
}
