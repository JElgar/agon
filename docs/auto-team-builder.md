# Auto team builder

Status: built (2026-11-15). The split runs client-side via `agon_core` compiled to wasm.
UI design canvas: https://claude.ai/artifact/AsppPerX1Afqbj2YDjBbpT

## What it is

An organiser has a pool of players and no fixed teams. They hit
**Auto-assign**, pick **Random** or **Auto Balanced**, preview the split,
re-roll or pin players as they like, then apply it. It's available on two
surfaces:

1. **Teamless tournaments** (`entry_audience: Players`, `team_formation:
   Manual`). It fills in each entry's `squad` before `start`, using the
   same `POST /tournaments/:id/entries/:entrant_id/squad` the board uses
   for drag-and-drop.
2. **Matches with a shared pool**: intra-squad matches (both sides the same
   team), or any match with unassigned players. It fills in `side_id` on
   the existing sides, through `PATCH /matches/:id` → `side_assignments`.

This is a one-off assistant, **not** the per-round re-draft that
`TournamentTeamFormationRecord::Auto { by }` describes. After it runs, the
tournament is an ordinary `Manual` one. A future `Auto` mode, or a
"suggest a side for this joiner" feature, can call the same `agon_core`
code from the backend.

## Product decisions (from the design review)

| # | Decision |
|---|---|
| 1 | Two methods only: **Random** and **Auto Balanced**. No "balance by" option. |
| 2 | Rating is trusted progressively: weight goes 0 → 100% over the first **10 ranked games**, with win rate covering the rest. |
| 3 | The preview shows each player's rating and/or win rate, **organiser only**. Players never see that squads were auto-built. |
| 4 | A rating still in placement (< 5 ranked) shows as **"Still in placements"**, with no count and no number. It still feeds the estimate. |
| 5 | `squad_count` defaults to `ceil(pool / squad_size)` and can be changed. |
| 6 | Re-roll and pins work for both methods. |
| 7 | Organiser override per player: **Newer / As expected / Stronger**, relative to *this group's* average, for this draw only. |
| 8 | No "drift band" visual on squads. Just say how many players have little history and that they're spread out. Two sides keep the predicted win bar and its "could be X–Y%" range. |

## Architecture

The split runs **in the browser**. The logic is Rust in the shared
`agon_core` crate, compiled to wasm, so the browser runs exactly the code
the tests cover.

```
agon_core::team_builder          (shared crate, pure: no I/O, no clock)
  ├── strength.rs   history → Estimate { mu, sigma, signal, certainty }
  ├── solver.rs     players + sides + method + seed → split + balance
  ├── plan.rs       one call: estimate, split, measure, label (serde in/out)
  └── scale.rs      the rating engine's constants + display ↔ native mapping

agon_core_wasm                   (thin wasm-bindgen shim)
  └── autoAssign(json) -> json   PlanInput in, PlanOutput out

agon_ui
  ├── lib/agonCore.ts            loads the wasm once; TS mirrors of the serde shapes
  └── components/agon/AutoAssignSheet.tsx
```

**Where the data comes from.** Everything is already public on
`GET /users/{id}`: a published rating's `rating`, `±` (`confidence`) and
`matches_rated` (the display form of `μ`, `σ` and the ranked-game count;
`scale::mu_from_display`/`sigma_from_display` invert it), plus the
per-sport played/won/drawn/lost. The sheet fetches each pool member's
profile once.

**Placement ratings** are never published, so the browser can't see them.
A player still in placements arrives as win-rate-only (the `Placement`
signal still exists in `agon_core` for any backend caller that can see
one). That's a small accuracy loss for those few players, and it means
there's nothing to withhold.

**Apply** uses endpoints that already exist:

- **Tournaments:** `POST /tournaments/{id}/entries/{entrant_id}/squad` per
  moved entrant, then a default label for any newly used squad (the board
  already does this).
- **Matches:** one `PATCH /matches/{id}` with every move in
  `side_assignments`.

Just before applying, the sheet re-reads the roster from the API. If
anyone joined, left or moved since the preview, it writes nothing and asks
for a fresh preview. The tournament path is one request per entrant, so a
failure partway through leaves some entrants moved. That's visible on the
board and safe to re-run.

**Constants.** `scale.rs` mirrors the rating engine's constants, because
the engine (`agon_services_core::rating`, built on `skillratings`) isn't
wasm-light. `agon_service`'s `team_builder_scale_matches_the_rating_engine`
test sees both crates and fails if they drift.

**Building.** `npm run build:wasm` in `agon_ui` builds `agon_core_wasm` for
`wasm32-unknown-unknown` and runs `wasm-bindgen --target web` into
`agon_ui/src/wasm/agon_core` (gitignored). It needs the wasm32 target and
`wasm-bindgen-cli` at exactly the version `agon_core_wasm` pins (0.2.100).
In CI, `build-ui.yml` builds the wasm and uploads it as an artifact, and
`docker-ui.yml` downloads it into the Docker build context, the same way
it handles the generated OpenAPI types.

The certainty dots are coarse and come from the signal: 3 for rated (10+
ranked games); 1 for a new account, a guest, or a win-rate-only player
with under 5 games; 2 for everyone else. The solver uses the continuous
`σ`, not the dots.

### Solver details

- **Random:** place the pinned players, shuffle the rest with the seed,
  deal into sides with room.
- **Auto Balanced:** snake draft by `μ`, then hill-climb pairwise swaps
  (pins never move). Two sides with ≤ 16 free players are solved exactly.
  The objective is lexicographic: minimise the `Σμ` spread, then among
  splits within tolerance minimise the `Σσ²` spread, so the unknowns end up
  spread out. Re-roll picks among near-best splits by seed.
- **Invariants** (unit-tested): everyone placed once, pins respected, sizes
  within one (except where pins force otherwise), caps never exceeded, same
  input and seed give the same output.
- **RNG:** `rand_chacha::ChaCha8Rng`, seeded by the caller. No OS
  randomness, so no `getrandom`, which is what keeps it wasm-buildable.

### Rules

- **Organiser-only** surfaces: tournament owner/admin on the entries page,
  match admins in the roster editor.
- **Tournaments:** while `open`, teamless, two or more accepted entrants.
- **Matches:** while `scheduled` (before the first live event or score),
  two or more sides. Neither checks the clock, so starting late is fine.

## Open items

- **Tuning constants** (`C_WR = 2.5`, `WR_SIGMA_FLOOR = 5.5`,
  `WR_SIGMA_HALF_GAMES = 10`, the solver tolerances) are hand-picked. See
  "Calibration" below.

## Later (not v1)

- Per-round re-draft (`Auto { by }` formation) on top of `solver`.
- Keep-together / keep-apart pairs: a "reject this swap" constraint in the
  hill-climb.
- Positions (GK, bowlers): needs position data first. It would then be a
  per-side quota constraint.
- Calibration: replay rated history to check that "even" splits produced
  close games, and tune the constants.
- Creating new match sides on the fly ("split these 15 into three").
