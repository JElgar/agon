import type { NetballFormat } from '@/lib/matchFormat'
import { isLivePlayPhase, liveClockLabel, phaseFromState, phaseLabel, type NetballScore } from '@/lib/netballScore'

/** "4 × 15 min quarters · 2-point zone · extra time" — the Match rules row. */
export function netballRulesSummary(fmt: NetballFormat): string {
  const parts = [`${fmt.num_quarters} × ${fmt.quarter_length_minutes} min quarters`]
  if (fmt.two_point_zone) parts.push('2-point zone')
  if (fmt.extra_time) parts.push('extra time')
  return parts.join(' · ')
}

/** "3rd quarter · 7'" while a quarter runs, else the phase ("Half-time"). */
export function netballLiveLabel(state: NetballScore): string {
  const phase = phaseFromState(state)
  const clock = liveClockLabel(state)
  if (!isLivePlayPhase(phase)) return phaseLabel(phase)
  return clock === 'LIVE' ? phaseLabel(phase) : `${phaseLabel(phase)} · ${clock}`
}
