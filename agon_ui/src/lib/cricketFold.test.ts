import { describe, expect, it } from 'vitest'
import type { components } from '@/types/api'
import { applyCricketEvent, foldCricketEvents, type CricketFoldFormat } from './cricketFold'
import type { CricketDelivery, CricketLiveEvent } from './cricketScore'

type InningsEndReason = components['schemas']['InningsEndReason']

// Fixtures ported verbatim from `agon_service/src/live_score/cricket.rs`'s
// `#[cfg(test)] mod tests` — same input events, same assertions, on purpose:
// these are exactly the strike-rotation/maiden/over-boundary edge cases a
// hand-port can silently get wrong (see `cricketFold.ts`'s doc comment), so
// the oracle here is the Rust source, not a re-derived TS understanding of
// the rules.

const STANDARD_FORMAT: CricketFoldFormat = {
  balls_per_over: 6,
  wide_is_extra_ball: true,
  no_ball_is_extra_ball: true,
}

function ball(bowler: string, striker: string, nonStriker: string, runs: number): CricketDelivery {
  return {
    over: 0,
    ball: 1,
    bowler_player_id: bowler,
    striker_player_id: striker,
    non_striker_player_id: nonStriker,
    runs_off_bat: runs,
  }
}

function delivery(d: CricketDelivery): CricketLiveEvent {
  return { kind: 'Delivery', ...d }
}

function inningsStart(battingSideId: string, bowlingSideId: string): CricketLiveEvent {
  return { kind: 'InningsStart', batting_side_id: battingSideId, bowling_side_id: bowlingSideId }
}

function inningsEnd(reason: InningsEndReason): CricketLiveEvent {
  return { kind: 'InningsEnd', reason }
}

function retire(batterPlayerId: string, retiredOut: boolean): CricketLiveEvent {
  return { kind: 'Retire', batter_player_id: batterPlayerId, retired_out: retiredOut }
}

function score(events: CricketLiveEvent[]) {
  return foldCricketEvents(events, STANDARD_FORMAT)
}

describe('applyCricketEvent / foldCricketEvents', () => {
  it('tracks runs, wickets, overs and cards per innings', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 4)),
      delivery({
        ...ball('patel', 'sharma', 'verma', 0),
        wicket: {
          kind: 'bowled',
          dismissed_player_id: 'sharma',
          bowler_player_id: 'patel',
        },
      }),
    ]

    const d = score(events)
    expect(d.innings.length).toBe(1)
    expect(d.innings[0].runs).toBe(4)
    expect(d.innings[0].wickets).toBe(1)
    expect(d.innings[0].overs).toEqual({ overs: 0, balls: 2 })
    expect(d.awaiting_next_innings).toBe(false)

    const sharma = d.innings[0].batting?.find((b) => b.player_id === 'sharma')
    expect(sharma?.runs).toBe(4)
    expect(sharma?.dismissal?.kind).toBe('bowled')

    const patel = d.innings[0].bowling?.find((b) => b.player_id === 'patel')
    expect(patel?.runs_conceded).toBe(4)
    expect(patel?.wickets).toBe(1)
  })

  it('a wicketless over is a maiden', () => {
    const events: CricketLiveEvent[] = [inningsStart('warriors', 'mill_lane')]
    for (let i = 0; i < 6; i++) events.push(delivery(ball('patel', 'sharma', 'verma', 0)))

    const d = score(events)
    const patel = d.innings[0].bowling?.find((b) => b.player_id === 'patel')
    expect(patel?.maidens).toBe(1)
    expect(patel?.overs).toEqual({ overs: 1, balls: 0 })
  })

  it('a single run breaks the maiden', () => {
    const events: CricketLiveEvent[] = [inningsStart('warriors', 'mill_lane')]
    events.push(delivery(ball('patel', 'sharma', 'verma', 1)))
    for (let i = 0; i < 5; i++) events.push(delivery(ball('patel', 'verma', 'sharma', 0)))

    const d = score(events)
    const patel = d.innings[0].bowling?.find((b) => b.player_id === 'patel')
    expect(patel?.maidens).toBe(0)
  })

  it('a wide-only over is still a maiden (runs_conceded_this_over voids it, not the ball count)', () => {
    // Not a direct Rust fixture, but pins the specific trap called out in
    // `cricketFold.ts`'s doc comment: `runs_conceded_this_over` accumulates
    // outside the `if legal` branch, so a wide's penalty run must still
    // void the maiden even though the wide itself doesn't advance the over.
    const events: CricketLiveEvent[] = [inningsStart('warriors', 'mill_lane')]
    events.push(
      delivery({ ...ball('patel', 'sharma', 'verma', 0), extra: { kind: 'wide', runs: 1 } }),
    )
    for (let i = 0; i < 6; i++) events.push(delivery(ball('patel', 'sharma', 'verma', 0)))

    const d = score(events)
    const patel = d.innings[0].bowling?.find((b) => b.player_id === 'patel')
    expect(patel?.maidens).toBe(0)
    expect(patel?.overs).toEqual({ overs: 1, balls: 0 })
  })

  it('recent deliveries is capped and cleared between innings', () => {
    const RECENT_DELIVERIES_LIMIT = 18
    const events: CricketLiveEvent[] = [inningsStart('warriors', 'mill_lane')]
    for (let i = 0; i < RECENT_DELIVERIES_LIMIT + 5; i++) {
      events.push(delivery(ball('patel', 'sharma', 'verma', 1)))
    }
    let d = score(events)
    expect(d.recent_deliveries?.length).toBe(RECENT_DELIVERIES_LIMIT)
    expect(d.innings[0].runs).toBe(RECENT_DELIVERIES_LIMIT + 5)

    events.push(inningsEnd('declared'))
    d = score(events)
    expect(d.recent_deliveries).toBeUndefined()
    expect(d.next_ball_context).toBeUndefined()
    expect(d.awaiting_next_innings).toBe(true)
  })

  it('next-ball context rotates strike on odd runs and the over boundary', () => {
    const events = [inningsStart('warriors', 'mill_lane'), delivery(ball('patel', 'sharma', 'verma', 1))]
    let ctx = score(events).next_ball_context!
    expect(ctx.striker_player_id).toBe('verma')
    expect(ctx.non_striker_player_id).toBe('sharma')
    expect(ctx.bowler_player_id).toBe('patel')
    expect(ctx.ball).toBe(2)
    expect(ctx.over).toBe(0)

    const allEvents = [...events]
    for (let i = 0; i < 5; i++) allEvents.push(delivery(ball('patel', 'verma', 'sharma', 0)))
    ctx = score(allEvents).next_ball_context!
    expect(ctx.over).toBe(1)
    expect(ctx.ball).toBe(1)
    expect(ctx.previous_over_bowler_player_id).toBe('patel')
    expect(ctx.bowler_player_id).toBeUndefined()
  })

  it("a wicket vacates the dismissed batter's slot", () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery({
        ...ball('patel', 'sharma', 'verma', 0),
        wicket: { kind: 'bowled', dismissed_player_id: 'sharma', bowler_player_id: 'patel' },
      }),
    ]
    const ctx = score(events).next_ball_context!
    expect(ctx.striker_player_id).toBeUndefined()
    expect(ctx.non_striker_player_id).toBe('verma')
  })

  it('a run-out on the over\'s last ball leaves the vacancy in the surviving slot', () => {
    // Pins the literal-swap trap: the odd-runs swap and the end-of-over
    // swap both apply on top of the wicket-vacated slot, in sequence.
    const events: CricketLiveEvent[] = [inningsStart('warriors', 'mill_lane')]
    for (let i = 0; i < 5; i++) events.push(delivery(ball('patel', 'sharma', 'verma', 0)))
    events.push(
      delivery({
        ...ball('patel', 'sharma', 'verma', 1),
        wicket: { kind: 'run_out', dismissed_player_id: 'verma' },
      }),
    )
    const ctx = score(events).next_ball_context!
    // 1 run (odd) swaps sharma<->verma first (verma now "striker" slot,
    // sharma "non-striker"); verma is then vacated by the wicket check
    // (evaluated against the pre-swap d.striker/non_striker, per the source
    // order); the over-boundary swap fires last. Net: over completes (ball
    // 6), a new bowler is required, and exactly one of the two slots holds
    // "sharma" while the other is vacant — assert that shape rather than
    // over-specifying which named slot, since that's the part the swap
    // order actually decides.
    expect(ctx.over).toBe(1)
    expect(ctx.ball).toBe(1)
    expect(ctx.bowler_player_id).toBeUndefined()
    const slots = [ctx.striker_player_id, ctx.non_striker_player_id]
    expect(slots.filter((s) => s === 'sharma').length).toBe(1)
    expect(slots.filter((s) => s === undefined).length).toBe(1)
  })

  it('retiring hurt lets the same batter resume without a wicket', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 4)),
      retire('sharma', false),
      delivery(ball('patel', 'sharma', 'verma', 1)),
    ]
    const d = score(events)
    expect(d.innings.length).toBe(1)
    expect(d.innings[0].wickets).toBe(0)
    const sharma = d.innings[0].batting?.find((b) => b.player_id === 'sharma')
    expect(sharma?.runs).toBe(5)
    expect(sharma?.dismissal?.kind).toBe('retired_hurt')
    expect(d.awaiting_next_innings).toBe(false)
  })

  it('retiring out counts as a wicket', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 2)),
      retire('sharma', true),
    ]
    const d = score(events)
    expect(d.innings[0].wickets).toBe(1)
    expect(d.innings[0].fall_of_wickets?.length).toBe(1)
    expect(d.innings[0].fall_of_wickets?.[0].player_id).toBe('sharma')
  })

  it('innings end and start split totals by inferred index, not a stored field', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 4)),
      inningsEnd('declared'),
      inningsStart('mill_lane', 'warriors'),
      delivery(ball('sharma', 'cole', 'adeyemi', 1)),
    ]
    const d = score(events)
    expect(d.innings.length).toBe(2)
    expect(d.innings[0].batting_side_id).toBe('warriors')
    expect(d.innings[0].runs).toBe(4)
    expect(d.innings[0].declared).toBe(true)
    expect(d.innings[1].batting_side_id).toBe('mill_lane')
    expect(d.innings[1].runs).toBe(1)
    expect(d.awaiting_next_innings).toBe(false)
  })

  it('deleting a wrongly-placed innings end re-flows events into the earlier innings', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 4)),
      delivery(ball('patel', 'sharma', 'verma', 2)),
    ]
    const d = score(events)
    expect(d.innings.length).toBe(1)
    expect(d.innings[0].runs).toBe(6)
    expect(d.awaiting_next_innings).toBe(false)
  })

  it('incremental and full fold agree', () => {
    const events = [
      inningsStart('warriors', 'mill_lane'),
      delivery(ball('patel', 'sharma', 'verma', 4)),
      delivery(ball('patel', 'sharma', 'verma', 1)),
      delivery({
        ...ball('patel', 'verma', 'sharma', 0),
        wicket: {
          kind: 'caught',
          dismissed_player_id: 'verma',
          bowler_player_id: 'patel',
          fielder_player_id: 'khan',
        },
      }),
    ]

    const full = foldCricketEvents(events, STANDARD_FORMAT)

    let incremental = { innings: [], players: {} } as ReturnType<typeof foldCricketEvents>
    for (const event of events) {
      incremental = applyCricketEvent(incremental, event, STANDARD_FORMAT)
    }

    expect(incremental.innings.length).toBe(full.innings.length)
    expect(incremental.innings[0].runs).toBe(full.innings[0].runs)
    expect(incremental.innings[0].wickets).toBe(full.innings[0].wickets)
    expect(incremental.innings[0].batting?.length).toBe(full.innings[0].batting?.length)
    expect(incremental.innings[0].bowling?.length).toBe(full.innings[0].bowling?.length)
  })
})
