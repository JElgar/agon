import type { CricketFormat } from '@/lib/matchFormat'

export type CricketTab = 'summary' | 'scorecard' | 'timeline' | 'players'
export const CRICKET_TABS: { id: CricketTab; label: string }[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'scorecard', label: 'Scorecard' },
  { id: 'timeline', label: 'Timeline' },
  { id: 'players', label: 'Players' },
]

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`
}

/** "20 overs · single innings · no-ball 1 run + free hit" — the Match rules row. */
export function cricketRulesSummary(fmt: CricketFormat): string {
  const parts = [fmt.overs_per_innings ? `${fmt.overs_per_innings} overs` : 'Unlimited overs']
  parts.push(fmt.innings_per_side === 1 ? 'single innings' : `${fmt.innings_per_side} innings each`)
  parts.push(`no-ball ${plural(fmt.no_ball_penalty_runs, 'run')}${fmt.free_hit_after_no_ball ? ' + free hit' : ''}`)
  return parts.join(' · ')
}
