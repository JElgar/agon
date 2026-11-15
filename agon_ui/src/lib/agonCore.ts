/**
 * The browser's door into `agon_core` (Rust, compiled to wasm by
 * `npm run build:wasm` from `agon_core_wasm`). The wasm module is loaded
 * once, on first use, so pages that never call into it don't pay for it.
 *
 * The bindings speak JSON in the serde shapes `agon_core` defines and
 * tests; the types below mirror them by hand. Keep them in step with
 * `agon_core/src/team_builder/plan.rs` — a mismatch shows up as a
 * "bad input" error from `autoAssign`, not silently.
 */
import init, { autoAssign as rawAutoAssign } from '@/wasm/agon_core/agon_core_wasm'

let ready: Promise<unknown> | null = null

/** Load (once) and initialise the wasm module. */
export function loadAgonCore(): Promise<unknown> {
  ready ??= init()
  return ready
}

export type AutoAssignMethod = 'random' | 'auto_balanced'
export type AutoAssignScope = 'unassigned' | 'everyone'
export type PlayerLevel = 'newer' | 'stronger'
export type StrengthSignal = 'rated' | 'blended' | 'placement' | 'win_rate' | 'new' | 'guest'
export type BalanceVerdict = 'even' | 'close' | 'uneven'

export interface PlanPlayer {
  id: string
  /** Published rating on this sport's ladder (`LadderRating`), if any. */
  rating?: { rating: number; confidence: number; matches_rated: number } | null
  record?: { played: number; wins: number; draws: number; losses: number }
  /** A roster entry with no account. */
  guest?: boolean
  level?: PlayerLevel | null
  /** Side index they're on now. */
  current_side?: number | null
  /** Side index the organiser pinned them to. */
  pinned_side?: number | null
}

export interface PlanInput {
  players: PlanPlayer[]
  /** One per side, in side order: max players, or null. */
  caps: (number | null)[]
  method: AutoAssignMethod
  scope: AutoAssignScope
  seed: number
}

export interface PlannedPlayer {
  id: string
  signal: StrengthSignal
  /** 1 = little history, 2 = some, 3 = solid. */
  certainty: number
  /** Display scale; null for a placement rating. */
  estimate: number | null
  ranked_games: number | null
  win_rate: { raw: number; adjusted: number; played: number } | null
  /** Share of the estimate from the rating, 0–1. */
  rating_weight: number
  level: PlayerLevel | null
  pinned: boolean
  /** Placed somewhere other than where they are now. */
  moved: boolean
}

export interface PlannedSide {
  /** Display scale, rounded to 10; null for an empty side. */
  average: number | null
  unsure_count: number
  players: PlannedPlayer[]
}

export interface PlanOutput {
  seed: number
  method: AutoAssignMethod
  verdict: BalanceVerdict
  /** Display points between the strongest and weakest side's average. */
  spread: number
  /** Two sides only: chance the first side wins, with a ±1σ range. */
  win_probability: { expected: number; low: number; high: number } | null
  unsure_count: number
  sides: PlannedSide[]
}

/** Split a pool into sides. Rejects with a readable message for a pool
 *  that doesn't fit the sides' caps. */
export async function autoAssign(input: PlanInput): Promise<PlanOutput> {
  await loadAgonCore()
  return JSON.parse(rawAutoAssign(JSON.stringify(input))) as PlanOutput
}

/** A fresh seed that's an exact integer in both JS and Rust. */
export function newSeed(): number {
  return Math.floor(Math.random() * 2 ** 31)
}
