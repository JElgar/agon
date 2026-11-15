/**
 * The browser's door into `agon_core` (Rust, compiled to wasm by
 * `npm run build:wasm` from `agon_core_wasm`). The wasm module is loaded
 * once, on first use, so pages that never call into it don't pay for it.
 *
 * Every type here is generated from the Rust structs (`agon_core`'s `wasm`
 * feature derives them with tsify into the package's `.d.ts`) — nothing is
 * hand-copied. Values cross as plain JS objects; an optional Rust field is
 * `T | undefined` on this side.
 */
import init, {
  autoAssign as rawAutoAssign,
  type Level,
  type Method,
  type PlanInput,
  type PlanOutput,
  type PlanPlayer,
  type PlannedPlayer,
  type PlannedSide,
  type Scope,
  type Signal,
  type Verdict,
} from '@/wasm/agon_core/agon_core_wasm'

export type { PlanInput, PlanOutput, PlanPlayer, PlannedPlayer, PlannedSide }
export type AutoAssignMethod = Method
export type AutoAssignScope = Scope
export type PlayerLevel = Level
export type StrengthSignal = Signal
export type BalanceVerdict = Verdict

let ready: Promise<unknown> | null = null

/** Load (once) and initialise the wasm module. */
export function loadAgonCore(): Promise<unknown> {
  ready ??= init()
  return ready
}

/** Split a pool into sides. Rejects with a readable message for a pool
 *  that doesn't fit the sides' caps. */
export async function autoAssign(input: PlanInput): Promise<PlanOutput> {
  await loadAgonCore()
  return rawAutoAssign(input)
}

/** A fresh seed that's an exact integer in both JS and Rust. */
export function newSeed(): number {
  return Math.floor(Math.random() * 2 ** 31)
}
