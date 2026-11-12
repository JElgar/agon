import { useMemo, useState } from 'react'
import {
  ArrowDown,
  ArrowUp,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Crown,
  Flame,
  MapPin,
  Medal,
  Plus,
  Swords,
  Trophy,
  UserPlus,
} from 'lucide-react'
import { Avatar } from '@/components/agon/Avatar'
import { Chip } from '@/components/agon/Chip'
import { StatTile } from '@/components/agon/StatTile'
import { Card } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { sportIcon, sportLabel, sportTint } from '@/lib/sports'
import { cn } from '@/lib/utils'
import {
  leaderboards,
  metrics,
  nextFixture,
  rivals,
  summaries,
  team,
  type DesignSport,
  type LeaderboardMetric,
  type LeaderboardRow,
  type Result,
  type TeamSummary,
} from './teamFixtures'

/**
 * DESIGN PROTOTYPE — fixture data only, mounted at `/design/team` in dev
 * builds (see `App.tsx`). Explores what a fuller team page could show:
 * a form guide, a team-stats overview, a player leaderboard scoped to this
 * team, rivals and the next fixture. See `docs/teams-page-scope.md` for the
 * backend each section would need.
 */

type Tab = 'overview' | 'leaderboard' | 'matches' | 'squad'

const TABS: { key: Tab; label: string }[] = [
  { key: 'overview', label: 'Overview' },
  { key: 'leaderboard', label: 'Leaderboard' },
  { key: 'matches', label: 'Matches' },
  { key: 'squad', label: 'Squad' },
]

export function TeamPageDesign() {
  const [sport, setSport] = useState<DesignSport>(team.sports[0])
  const [tab, setTab] = useState<Tab>('overview')
  const summary = summaries[sport]

  return (
    <div className="min-h-screen bg-background">
      <DesignBanner />
      <div className="container mx-auto px-4 py-6 pb-24">
        <div className="mx-auto flex max-w-xl flex-col gap-5 md:max-w-3xl md:gap-6 lg:max-w-6xl">
          <Header />

          <FormStrip summary={summary} />

          {/* Sport switcher — only when the team plays more than one. Drives
              every section below, since a cricket record and a football
              record don't add up into anything meaningful. */}
          <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
            <nav className="flex gap-0.5 rounded-full bg-muted p-1" aria-label="Team sections">
              {TABS.map((t) => (
                <button
                  key={t.key}
                  type="button"
                  onClick={() => setTab(t.key)}
                  aria-current={tab === t.key ? 'page' : undefined}
                  className={cn(
                    'h-9 flex-1 whitespace-nowrap rounded-full px-2.5 text-sm sm:px-4 font-semibold transition-colors md:flex-none',
                    tab === t.key
                      ? 'bg-card text-foreground shadow-sm'
                      : 'text-muted-foreground hover:text-foreground',
                  )}
                >
                  {t.label}
                </button>
              ))}
            </nav>
            {team.sports.length > 1 && (
              <div className="flex gap-2">
                {team.sports.map((s) => {
                  const Icon = sportIcon(s)
                  return (
                    <Chip
                      key={s}
                      pressed={sport === s}
                      onClick={() => setSport(s)}
                      className="flex items-center gap-1.5"
                    >
                      <Icon className="size-4" />
                      {sportLabel(s)}
                    </Chip>
                  )
                })}
              </div>
            )}
          </div>

          {tab === 'overview' && (
            <Overview sport={sport} summary={summary} onSeeLeaderboard={() => setTab('leaderboard')} />
          )}
          {tab === 'leaderboard' && <Leaderboard sport={sport} />}
          {tab === 'matches' && <Matches summary={summary} />}
          {tab === 'squad' && <Squad sport={sport} />}
        </div>
      </div>
    </div>
  )
}

function DesignBanner() {
  return (
    <div className="border-b border-dashed bg-warning/10 px-4 py-2 text-center text-xs font-medium text-warning-foreground">
      Design prototype · fixture data · <code>/design/team</code>
    </div>
  )
}

// ---- Header ----------------------------------------------------------------

function Header() {
  return (
    <div className="flex flex-col gap-4">
      <Button variant="ghost" size="icon" className="size-11 rounded-full" aria-label="Back">
        <ChevronLeft className="size-5" />
      </Button>
      <div className="flex flex-col gap-4 md:flex-row md:items-center md:justify-between">
        <div className="flex items-center gap-4">
          <Avatar
            name={team.name}
            size="xl"
            className="size-[76px] rounded-[20px] bg-primary text-2xl text-primary-foreground"
          />
          <div className="min-w-0">
            <h1 className="truncate font-display text-2xl font-extrabold md:text-3xl">{team.name}</h1>
            <div className="mt-1 flex flex-wrap items-center gap-1.5">
              {team.sports.map((s) => (
                <SportPill key={s} sport={s} />
              ))}
            </div>
            <p className="mt-1 text-sm text-muted-foreground">
              <span className="font-bold text-foreground">{team.followers}</span> followers ·{' '}
              <span className="font-bold text-foreground">{team.members}</span> members · since {team.founded}
            </p>
          </div>
        </div>
        <div className="flex gap-2 md:shrink-0">
          <Button className="flex-1 gap-2 rounded-2xl md:flex-none">
            <Plus className="size-4" />
            Log match
          </Button>
          <Button variant="outline" className="flex-1 gap-2 rounded-2xl md:flex-none">
            <UserPlus className="size-4" />
            Invite
          </Button>
        </div>
      </div>
    </div>
  )
}

function SportPill({ sport }: { sport: DesignSport }) {
  const tint = sportTint(sport)
  const Icon = sportIcon(sport)
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-semibold"
      style={{ background: tint.bg, color: tint.fg }}
    >
      <Icon className="size-3" />
      {sportLabel(sport)}
    </span>
  )
}

// ---- Form strip -----------------------------------------------------------

const RESULT_STYLE: Record<Result, string> = {
  W: 'bg-success text-success-foreground',
  D: 'bg-muted-foreground/70 text-background',
  L: 'bg-destructive text-destructive-foreground',
}

function ResultDot({ result, size = 'md' }: { result: Result; size?: 'sm' | 'md' }) {
  return (
    <span
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-lg font-display font-extrabold',
        size === 'md' ? 'size-8 text-sm' : 'size-6 text-xs',
        RESULT_STYLE[result],
      )}
    >
      {result}
    </span>
  )
}

/** Always-visible "how are we doing right now": last five, plus the streak. */
function FormStrip({ summary }: { summary: TeamSummary }) {
  const { currentStreak } = summary
  const streakWord = { W: 'win', D: 'draw', L: 'loss' }[currentStreak.kind]
  return (
    <Card className="flex items-center justify-between gap-3 px-4 py-3">
      <div className="flex flex-col gap-1.5">
        <span className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Form</span>
        <div className="flex gap-1.5">
          {summary.form.slice(0, 5).map((m, i) => (
            <span key={i} title={`${m.result} ${m.score} vs ${m.opponent}`}>
              <ResultDot result={m.result} />
            </span>
          ))}
        </div>
      </div>
      {currentStreak.length >= 2 && (
        <div className="flex items-center gap-2 rounded-2xl bg-accent px-3 py-2">
          <Flame className="size-5 text-primary" />
          <div className="leading-tight">
            <div className="font-display text-lg font-extrabold">{currentStreak.length}</div>
            <div className="text-xs text-muted-foreground">{streakWord} streak</div>
          </div>
        </div>
      )}
    </Card>
  )
}

// ---- Overview --------------------------------------------------------------

function Overview({
  sport,
  summary,
  onSeeLeaderboard,
}: {
  sport: DesignSport
  summary: TeamSummary
  onSeeLeaderboard: () => void
}) {
  return (
    <div className="flex flex-col gap-5 md:grid md:grid-cols-2 md:items-start lg:grid-cols-3">
      <div className="flex flex-col gap-5 lg:col-span-2">
        <RecordBanner summary={summary} />
        <TopPerformers sport={sport} />
        <section className="flex flex-col gap-2.5">
          <SectionHeading title="Leaderboard" action="See all" onAction={onSeeLeaderboard} />
          <LeaderboardTable sport={sport} metric={metrics[sport][0]} rows={sortRows(leaderboards[sport], metrics[sport][0]).slice(0, 5)} compact />
        </section>
      </div>
      <div className="flex flex-col gap-5">
        <NextFixture />
        <TrendChart summary={summary} />
        <Milestones summary={summary} />
        <Rivals sport={sport} />
      </div>
    </div>
  )
}

function SectionHeading({
  title,
  action,
  onAction,
}: {
  title: string
  action?: string
  onAction?: () => void
}) {
  return (
    <div className="flex items-center justify-between px-1">
      <h2 className="font-display text-lg font-bold">{title}</h2>
      {action && (
        <button
          type="button"
          onClick={onAction}
          className="flex items-center gap-0.5 text-sm font-semibold text-link"
        >
          {action}
          <ChevronRight className="size-4" />
        </button>
      )}
    </div>
  )
}

function RecordBanner({ summary }: { summary: TeamSummary }) {
  const total = summary.played || 1
  return (
    <section
      aria-label="Team record"
      className="flex flex-col gap-4 rounded-2xl border border-banner-tint-border bg-banner-tint p-[18px]"
    >
      <div className="grid grid-cols-3 gap-2">
        <StatTile tone="banner-blue" value={summary.played} label="Played" />
        <StatTile
          tone="banner-blue"
          value={
            <span>
              {summary.wins}
              <span className="text-banner-tint-muted-foreground">–{summary.draws}–{summary.losses}</span>
            </span>
          }
          label="W–D–L"
        />
        <StatTile tone="banner-blue" value={`${Math.round(summary.winRatePct)}%`} label="Win rate" />
      </div>
      {/* W/D/L as one proportional bar — reads faster than three numbers. */}
      <div className="flex h-2 overflow-hidden rounded-full bg-white/60">
        <div className="bg-success" style={{ width: `${(summary.wins / total) * 100}%` }} />
        <div className="bg-muted-foreground/50" style={{ width: `${(summary.draws / total) * 100}%` }} />
        <div className="bg-destructive" style={{ width: `${(summary.losses / total) * 100}%` }} />
      </div>
      <div className="grid grid-cols-2 gap-x-2 gap-y-3 border-t border-banner-tint-border pt-4 sm:grid-cols-4">
        {summary.extras.map((e) => (
          <div key={e.label} className="flex flex-col">
            <span className="font-display text-xl font-extrabold leading-tight">{e.value}</span>
            <span className="text-xs text-banner-tint-muted-foreground">
              {e.label}
              {e.hint && <span className="block opacity-80">{e.hint}</span>}
            </span>
          </div>
        ))}
      </div>
    </section>
  )
}

/** The dark "spotlight" card, one tile per headline category. */
function TopPerformers({ sport }: { sport: DesignSport }) {
  const rows = leaderboards[sport]
  const picks =
    sport === 'football'
      ? [
          { label: 'Top scorer', metric: 'goals', suffix: 'goals' },
          { label: 'Most assists', metric: 'assists', suffix: 'assists' },
          { label: 'Highest rated', metric: 'rating', suffix: '' },
        ]
      : [
          { label: 'Most runs', metric: 'runs', suffix: 'runs' },
          { label: 'Most wickets', metric: 'wickets', suffix: 'wkts' },
          { label: 'Highest rated', metric: 'rating', suffix: '' },
        ]

  return (
    <section aria-label="Top performers" className="rounded-2xl bg-spotlight p-4 text-spotlight-foreground">
      <div className="mb-3 flex items-center gap-2">
        <Trophy className="size-4 text-gold" />
        <h2 className="font-display text-base font-bold">Top performers</h2>
        <span className="ml-auto text-xs text-spotlight-muted">This season</span>
      </div>
      <div className="grid grid-cols-3 gap-2">
        {picks.map((p) => {
          const best = [...rows]
            .filter((r) => r.stats[p.metric] !== undefined)
            .sort((a, b) => b.stats[p.metric] - a.stats[p.metric])[0]
          return (
            <div key={p.label} className="flex flex-col items-center gap-2 rounded-xl bg-white/5 px-2 py-3 text-center">
              <Avatar name={best.name} size="lg" className="ring-2 ring-gold ring-offset-2 ring-offset-spotlight" />
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold">{best.name.split(' ')[0]}</div>
                <div className="font-display text-2xl font-extrabold leading-none">{best.stats[p.metric]}</div>
                <div className="mt-1 text-[11px] text-spotlight-muted">
                  {p.label}
                  {p.suffix && ` · ${p.suffix}`}
                </div>
              </div>
            </div>
          )
        })}
      </div>
    </section>
  )
}

function NextFixture() {
  const f = nextFixture
  return (
    <section className="flex flex-col gap-2.5">
      <SectionHeading title="Next up" />
      <Card className="flex flex-col gap-3 p-4">
        <div className="flex items-center gap-3">
          <Avatar name={team.name} size="lg" className="rounded-xl bg-primary text-primary-foreground" />
          <span className="text-sm font-semibold text-muted-foreground">vs</span>
          <Avatar name={f.opponent} size="lg" className="rounded-xl" />
          <div className="min-w-0 flex-1">
            <div className="truncate font-bold">{f.opponent}</div>
            <div className="flex items-center gap-1 text-xs text-muted-foreground">
              <CalendarDays className="size-3" /> {f.when}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <MapPin className="size-3" /> {f.where}
        </div>
        <div>
          <div className="mb-1 flex justify-between text-xs">
            <span className="font-semibold">{f.confirmed}/{f.needed} confirmed</span>
            <span className="text-muted-foreground">{f.needed - f.confirmed} spots left</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-muted">
            <div className="h-full rounded-full bg-primary" style={{ width: `${(f.confirmed / f.needed) * 100}%` }} />
          </div>
        </div>
        <Button size="sm" shape="pill" className="self-start">I'm in</Button>
      </Card>
    </section>
  )
}

/** Scored vs conceded per match, last 10 — a mirrored bar chart. */
function TrendChart({ summary }: { summary: TeamSummary }) {
  const max = Math.max(...summary.trend.flatMap((t) => [t.forValue, t.againstValue]), 1)
  return (
    <section className="flex flex-col gap-2.5">
      <SectionHeading title="Last 10" />
      <Card className="p-4">
        <div className="mb-3 flex gap-4 text-xs text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <span className="size-2.5 rounded-sm bg-link" /> {summary.trendLabel.forLabel}
          </span>
          <span className="flex items-center gap-1.5">
            <span className="size-2.5 rounded-sm bg-muted-foreground/40" /> {summary.trendLabel.againstLabel}
          </span>
        </div>
        <div className="flex h-28 items-stretch gap-1.5" role="img" aria-label="Scored versus conceded, last ten matches">
          {summary.trend.map((t, i) => (
            <div key={i} className="flex flex-1 flex-col items-center">
              <div className="flex w-full flex-1 items-end justify-center">
                <div className="w-full max-w-4 rounded-t-sm bg-link" style={{ height: `${(t.forValue / max) * 100}%` }} title={`${t.forValue}`} />
              </div>
              <div className="h-px w-full bg-border" />
              <div className="flex w-full flex-1 items-start justify-center">
                <div className="w-full max-w-4 rounded-b-sm bg-muted-foreground/40" style={{ height: `${(t.againstValue / max) * 100}%` }} title={`${t.againstValue}`} />
              </div>
            </div>
          ))}
        </div>
        <div className="mt-2 flex gap-1.5">
          {summary.trend.map((t, i) => (
            <div key={i} className="flex flex-1 justify-center">
              <span className={cn('size-1.5 rounded-full', t.result === 'W' ? 'bg-success' : t.result === 'L' ? 'bg-destructive' : 'bg-muted-foreground/60')} />
            </div>
          ))}
        </div>
      </Card>
    </section>
  )
}

function Milestones({ summary }: { summary: TeamSummary }) {
  const items = [
    { icon: Trophy, label: 'Biggest win', value: summary.biggestWin.score, sub: `vs ${summary.biggestWin.opponent} · ${summary.biggestWin.date}` },
    { icon: Flame, label: 'Longest win streak', value: `${summary.longestWinStreak}`, sub: 'matches in a row' },
  ]
  return (
    <section className="flex flex-col gap-2.5">
      <SectionHeading title="Records" />
      <Card className="flex flex-col divide-y">
        {items.map(({ icon: Icon, label, value, sub }) => (
          <div key={label} className="flex items-center gap-3 px-4 py-3">
            <span className="flex size-10 items-center justify-center rounded-full bg-accent">
              <Icon className="size-4 text-accent-foreground" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-xs text-muted-foreground">{label}</div>
              <div className="truncate text-sm font-semibold">{sub}</div>
            </div>
            <span className="font-display text-lg font-extrabold">{value}</span>
          </div>
        ))}
      </Card>
    </section>
  )
}

function Rivals({ sport }: { sport: DesignSport }) {
  return (
    <section className="flex flex-col gap-2.5">
      <SectionHeading title="Rivals" />
      <Card className="flex flex-col divide-y">
        {rivals[sport].map((r) => (
          <div key={r.name} className="flex items-center gap-3 px-4 py-3">
            <Avatar name={r.name} size="lg" className="rounded-xl" />
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold">{r.name}</div>
              <div className="text-xs text-muted-foreground">{r.played} meetings</div>
            </div>
            <div className="flex items-center gap-1 text-sm font-bold">
              <Swords className="mr-1 size-3.5 text-muted-foreground" />
              <span className="tabular-nums">
                <span className="text-success">{r.w}</span>
                <span className="text-muted-foreground">–{r.d}–</span>
                <span className="text-destructive">{r.l}</span>
              </span>
            </div>
          </div>
        ))}
      </Card>
    </section>
  )
}

// ---- Leaderboard -----------------------------------------------------------

type Scope = 'team' | 'career'
type Window = 'season' | 'all'

function sortRows(rows: LeaderboardRow[], metric: LeaderboardMetric, minApps = 0) {
  return [...rows]
    .filter((r) => r.stats[metric.key] !== undefined)
    .filter((r) => r.apps >= minApps)
    .sort((a, b) =>
      metric.ascending
        ? a.stats[metric.key] - b.stats[metric.key]
        : b.stats[metric.key] - a.stats[metric.key],
    )
}

/** Rate metrics are misleading on tiny samples — gate them on appearances. */
const RATE_METRICS = new Set(['winPct', 'gpm', 'avg', 'econ'])
const MIN_APPS_FOR_RATES = 5

function Leaderboard({ sport }: { sport: DesignSport }) {
  const sportMetrics = metrics[sport]
  const [metricKey, setMetricKey] = useState(sportMetrics[1].key)
  const [scope, setScope] = useState<Scope>('team')
  const [window, setWindow] = useState<Window>('season')
  const metric = sportMetrics.find((m) => m.key === metricKey) ?? sportMetrics[0]
  const minApps = RATE_METRICS.has(metric.key) ? MIN_APPS_FOR_RATES : 0
  const rows = useMemo(() => sortRows(leaderboards[sport], metric, minApps), [sport, metric, minApps])
  const unrated = metric.key === 'rating' ? leaderboards[sport].filter((r) => r.rating === undefined) : []

  return (
    <div className="flex flex-col gap-4 md:grid md:grid-cols-[1fr_280px] md:items-start md:gap-6">
      <div className="flex flex-col gap-4">
        <div className="flex gap-2 overflow-x-auto pb-1">
          {sportMetrics.map((m) => (
            <Chip key={m.key} pressed={m.key === metric.key} onClick={() => setMetricKey(m.key)}>
              {m.label}
            </Chip>
          ))}
        </div>

        {rows.length >= 3 && <Podium rows={rows.slice(0, 3)} metric={metric} />}

        <LeaderboardTable sport={sport} metric={metric} rows={rows} startRank={1} />

        {minApps > 0 && (
          <p className="px-1 text-xs text-muted-foreground">
            Rates only rank players with {MIN_APPS_FOR_RATES}+ appearances.
          </p>
        )}
        {unrated.length > 0 && (
          <p className="px-1 text-xs text-muted-foreground">
            {unrated.length} {unrated.length === 1 ? 'player is' : 'players are'} still in placements and
            not shown: {unrated.map((r) => r.name.split(' ')[0]).join(', ')}.
          </p>
        )}
      </div>

      <aside className="flex flex-col gap-4">
        <Card className="flex flex-col gap-3 p-4">
          <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Count</div>
          <Segmented
            value={scope}
            onChange={setScope}
            options={[
              { value: 'team', label: 'For this team' },
              { value: 'career', label: 'Career' },
            ]}
          />
          <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Period</div>
          <Segmented
            value={window}
            onChange={setWindow}
            options={[
              { value: 'season', label: 'This season' },
              { value: 'all', label: 'All time' },
            ]}
          />
          <p className="text-xs text-muted-foreground">
            <strong>For this team</strong> counts only matches played on a {team.name} side.{' '}
            <strong>Career</strong> uses each player's lifetime stats (available today).
          </p>
        </Card>
        <Card className="flex flex-col gap-2 p-4">
          <div className="flex items-center gap-2">
            <Medal className="size-4 text-gold" />
            <span className="text-sm font-bold">Badges this season</span>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {leaderboards[sport].flatMap((r) =>
              (r.badges ?? []).map((b) => (
                <span key={r.userId + b} className="rounded-full bg-accent px-2.5 py-1 text-xs font-medium">
                  {b} · {r.name.split(' ')[0]}
                </span>
              )),
            )}
          </div>
        </Card>
      </aside>
    </div>
  )
}

function Segmented<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T
  onChange: (v: T) => void
  options: { value: T; label: string }[]
}) {
  return (
    <div className="flex rounded-full bg-muted p-1">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={cn(
            'h-8 flex-1 rounded-full text-xs font-semibold transition-colors',
            value === o.value ? 'bg-card shadow-sm' : 'text-muted-foreground',
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

function formatMetric(metric: LeaderboardMetric, v: number) {
  return metric.format ? metric.format(v) : v.toLocaleString()
}

/** Top three, 2-1-3 like a real podium. */
function Podium({ rows, metric }: { rows: LeaderboardRow[]; metric: LeaderboardMetric }) {
  const order = [1, 0, 2]
  const heights = ['h-20', 'h-28', 'h-16']
  return (
    <Card className="flex items-end justify-center gap-3 px-4 pt-5">
      {order.map((idx, i) => {
        const row = rows[idx]
        return (
          <div key={row.userId} className="flex w-24 flex-col items-center gap-1.5">
            {idx === 0 && <Crown className="size-5 text-gold" />}
            <Avatar
              name={row.name}
              size={idx === 0 ? "xl" : "lg"}
              className={cn(idx === 0 ? 'ring-2 ring-gold ring-offset-2 ring-offset-card' : 'size-12 text-sm')}
            />
            <div className="w-full truncate text-center text-sm font-semibold">
              {row.name.split(' ')[0]}
              {row.isMe && <span className="text-muted-foreground"> (you)</span>}
            </div>
            <div className="font-display text-xl font-extrabold leading-none">
              {formatMetric(metric, row.stats[metric.key])}
            </div>
            <div
              className={cn(
                'mt-1 flex w-full items-start justify-center rounded-t-xl pt-2 font-display text-lg font-extrabold',
                heights[i],
                idx === 0 ? 'bg-primary text-primary-foreground' : 'bg-accent text-accent-foreground',
              )}
            >
              {idx + 1}
            </div>
          </div>
        )
      })}
    </Card>
  )
}

function LeaderboardTable({
  sport,
  metric,
  rows,
  startRank = 1,
  compact = false,
}: {
  sport: DesignSport
  metric: LeaderboardMetric
  rows: LeaderboardRow[]
  startRank?: number
  compact?: boolean
}) {
  // Two context columns beside the ranked metric: appearances always, plus
  // one sport-relevant pairing so the number isn't read in isolation.
  const secondary = sport === 'football' ? (metric.key === 'goals' ? 'assists' : 'goals') : metric.key === 'runs' ? 'wickets' : 'runs'
  const secondaryMetric = metrics[sport].find((m) => m.key === secondary)!

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center gap-3 border-b px-4 py-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
        <span className="w-6 text-center">#</span>
        <span className="flex-1">Player</span>
        {!compact && <span className="hidden w-10 text-right sm:block">Apps</span>}
        <span className="w-10 text-right">{secondaryMetric.short}</span>
        <span className="w-14 text-right text-foreground">{metric.short}</span>
      </div>
      <ol className="divide-y">
        {rows.map((row, i) => (
          <li
            key={row.userId}
            className={cn('flex items-center gap-3 px-4 py-2.5', row.isMe && 'bg-banner-tint')}
          >
            <span className="flex w-6 flex-col items-center">
              <span className="font-display text-base font-extrabold">{startRank + i}</span>
              <Movement value={row.movement} />
            </span>
            <Avatar name={row.name} size="lg" />
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="truncate text-sm font-semibold">{row.name}</span>
                {row.isMe && <span className="rounded-full bg-primary px-1.5 py-px text-[10px] font-bold text-primary-foreground">You</span>}
                {row.role === 'owner' && <Crown className="size-3.5 shrink-0 text-gold" aria-label="Owner" />}
              </div>
              <div className="text-xs text-muted-foreground">
                {row.wins}W in {row.apps} · {Math.round(row.winPct)}%
              </div>
            </div>
            {!compact && <span className="hidden w-10 text-right text-sm tabular-nums text-muted-foreground sm:block">{row.apps}</span>}
            <span className="w-10 text-right text-sm tabular-nums text-muted-foreground">
              {row.stats[secondary] !== undefined ? formatMetric(secondaryMetric, row.stats[secondary]) : '–'}
            </span>
            <span className="w-14 text-right font-display text-lg font-extrabold tabular-nums">
              {formatMetric(metric, row.stats[metric.key])}
            </span>
          </li>
        ))}
      </ol>
    </Card>
  )
}

function Movement({ value }: { value: number }) {
  if (value === 0) return <span className="h-3 text-[10px] leading-3 text-muted-foreground">–</span>
  const up = value > 0
  const Icon = up ? ArrowUp : ArrowDown
  return (
    <span className={cn('flex h-3 items-center text-[10px] font-bold leading-3', up ? 'text-success' : 'text-destructive')}>
      <Icon className="size-2.5" />
      {Math.abs(value)}
    </span>
  )
}

// ---- Matches / Squad (light — these exist today, kept for context) --------

function Matches({ summary }: { summary: TeamSummary }) {
  return (
    <Card className="flex flex-col divide-y">
      {summary.form.map((m, i) => (
        <div key={i} className="flex items-center gap-3 px-4 py-3">
          <ResultDot result={m.result} size="sm" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">vs {m.opponent}</div>
            <div className="text-xs text-muted-foreground">{m.date}</div>
          </div>
          <span className="font-display font-extrabold">{m.score}</span>
          <ChevronRight className="size-4 text-muted-foreground" />
        </div>
      ))}
    </Card>
  )
}

function Squad({ sport }: { sport: DesignSport }) {
  return (
    <Card className="flex flex-col divide-y">
      {leaderboards[sport].map((r) => (
        <div key={r.userId} className="flex items-center gap-3 px-4 py-3">
          <Avatar name={r.name} size="lg" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">{r.name}</div>
            <div className="text-xs capitalize text-muted-foreground">{r.role ?? 'member'} · {r.apps} apps</div>
          </div>
          {r.rating && <span className="text-sm font-bold tabular-nums">{r.rating}</span>}
        </div>
      ))}
    </Card>
  )
}
