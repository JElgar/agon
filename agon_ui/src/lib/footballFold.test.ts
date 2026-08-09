import { describe, expect, it } from 'vitest'
import { applyFootballEvent, foldFootballEvents, openingFootballScore, type FootballLiveEvent } from './footballFold'

// Fixtures ported verbatim from `agon_service/src/live_score/football.rs`'s
// `#[cfg(test)] mod tests` — same reasoning as `cricketFold.test.ts`: the
// Rust source is the oracle, not a re-derived TS understanding of the rules.

function ts(minute: number): string {
  return new Date((1_700_000_000 + minute * 60) * 1000).toISOString()
}

function events(...items: [number, FootballLiveEvent][]) {
  return items.map(([minute, event]) => ({ occurred_at: ts(minute), event }))
}

describe('applyFootballEvent / foldFootballEvents', () => {
  it('derives score, goals, cards and substitutions', () => {
    const log = events(
      [
        41,
        {
          kind: 'Goal',
          side_id: 'riverside',
          scorer_player_id: 'alvarez',
          assist_player_id: 'diaz',
          own_goal: false,
          penalty: false,
          minute: 41,
        },
      ],
      [58, { kind: 'Card', side_id: 'oak_park', player_id: 'khan', color: 'yellow', minute: 58 }],
      // An own goal counts for the side benefiting from it.
      [
        63,
        {
          kind: 'Goal',
          side_id: 'riverside',
          own_goal: true,
          penalty: false,
          minute: 63,
        },
      ],
      [
        70,
        {
          kind: 'Substitution',
          side_id: 'oak_park',
          player_in_id: 'moreno',
          player_out_id: 'khan',
          minute: 70,
        },
      ],
      [94, { kind: 'Period', period: 'full_time' }],
    )

    const score = foldFootballEvents(log)

    expect(Object.keys(score.score).length).toBe(1)
    expect(score.score.riverside).toBe(2)
    expect(score.goals?.length).toBe(2)
    expect(score.cards?.length).toBe(1)
    expect(score.substitutions?.length).toBe(1)
    expect(score.period).toBe('full_time')
    expect(score.period_times?.full_time).toBe(ts(94))
    expect(score.goals?.[1].own_goal).toBe(true)
    expect(score.substitutions?.[0].player_out_id).toBe('khan')
  })

  it('derives phase timestamps from period markers', () => {
    const log = events(
      [0, { kind: 'Period', period: 'kick_off' }],
      [46, { kind: 'Period', period: 'half_time' }],
      [60, { kind: 'Period', period: 'second_half_kick_off' }],
    )

    const score = foldFootballEvents(log)
    expect(score.period_times?.kick_off).toBe(ts(0))
    expect(score.period_times?.half_time).toBe(ts(46))
    expect(score.period_times?.second_half_kick_off).toBe(ts(60))
    expect(score.period_times?.full_time).toBeUndefined()
    expect(score.period).toBe('second_half_kick_off')
  })

  it('extra time and penalties markers get timestamps too', () => {
    const log = events(
      [90, { kind: 'Period', period: 'extra_time_kick_off' }],
      [105, { kind: 'Period', period: 'extra_time_half_time' }],
      [120, { kind: 'Period', period: 'extra_time_second_half_kick_off' }],
      [150, { kind: 'Period', period: 'extra_time_full_time' }],
      [160, { kind: 'Period', period: 'penalties_complete' }],
    )

    const score = foldFootballEvents(log)
    expect(score.period_times?.extra_time_kick_off).toBe(ts(90))
    expect(score.period_times?.extra_time_half_time).toBe(ts(105))
    expect(score.period_times?.extra_time_second_half_kick_off).toBe(ts(120))
    expect(score.period_times?.extra_time_full_time).toBe(ts(150))
    expect(score.period_times?.penalties_complete).toBe(ts(160))
  })

  it('penalty shootout kicks tally scored kicks only', () => {
    const log = events(
      [160, { kind: 'PenaltyShootoutKick', side_id: 'riverside', scored: true }],
      [161, { kind: 'PenaltyShootoutKick', side_id: 'oak_park', scored: false }],
      [162, { kind: 'PenaltyShootoutKick', side_id: 'riverside', scored: true }],
      [163, { kind: 'PenaltyShootoutKick', side_id: 'oak_park', scored: true }],
    )

    const score = foldFootballEvents(log)
    expect(score.penalty_shootout?.length).toBe(4)
    // Only sides with at least one scored kick get a tally entry — same
    // "absence means zero" convention as `score`.
    expect(Object.keys(score.penalty_shootout_score ?? {}).length).toBe(2)
    expect(score.penalty_shootout_score?.riverside).toBe(2)
    expect(score.penalty_shootout_score?.oak_park).toBe(1)
    // A shootout kick never counts as a match goal.
    expect(Object.keys(score.score).length).toBe(0)
    expect(score.goals?.length).toBe(0)
  })

  it('incremental and full fold agree', () => {
    const log = events(
      [0, { kind: 'Period', period: 'kick_off' }],
      [
        12,
        {
          kind: 'Goal',
          side_id: 'riverside',
          scorer_player_id: 'alvarez',
          own_goal: false,
          penalty: false,
          minute: 12,
        },
      ],
      [58, { kind: 'Card', side_id: 'oak_park', player_id: 'khan', color: 'yellow', minute: 58 }],
    )

    const full = foldFootballEvents(log)

    let incremental = openingFootballScore()
    for (const { occurred_at, event } of log) {
      incremental = applyFootballEvent(incremental, occurred_at, event)
    }

    expect(Object.keys(incremental.score).length).toBe(Object.keys(full.score).length)
    expect(incremental.goals?.length).toBe(full.goals?.length)
    expect(incremental.cards?.length).toBe(full.cards?.length)
    expect(incremental.period_times).toEqual(full.period_times)
  })
})
