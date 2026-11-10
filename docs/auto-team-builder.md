# Auto team builder

Status: design agreed, backend scoped (2026-11-10). Nothing is built yet.
UI design canvas: https://claude.ai/artifact/AsppPerX1Afqbj2YDjBbpT

## What it is

An organiser has a pool of players and no fixed teams. They hit
**Auto-assign**, pick **Random** or **Auto Balanced**, preview the split,
re-roll or pin players as they like, then apply it. It's available on two
surfaces:

1. **Teamless tournaments** (`entry_audience: Players`, `team_formation:
   Manual`). It fills in each entry's `squad` before `start`. Today every
   entrant is assigned by hand via
   `POST /tournaments/:id/entries/:entrant_id/squad`.
2. **Matches with a shared pool**: intra-squad matches (both sides the same
   team), or any match with unassigned players. It fills in `side_id` on
   the existing sides. Today that's `PATCH /matches/:id` →
   `side_assignments`, one player at a time.

This is a one-off assistant, **not** the per-round re-draft that
`TournamentTeamFormationRecord::Auto { by }` describes. After it runs, the
tournament is an ordinary `Manual` one. A future `Auto` mode can call the
same core module once per round.

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
| 8 | No "drift band" visual. Just say how many players have little history and that they're spread out. |

## Backend architecture

```
agon_core::team_builder          (new, pure: no DAO, no clock, no ids)
  ├── strength.rs   profile + sport → Estimate { mu, sigma, signal }
  └── solver.rs     players + sides + method + seed → Assignment

agon_core::dao                   (two new bulk writes)
  ├── match_ops.rs       reassign_match_players_tx
  └── tournament_ops.rs  set_tournament_entry_squads

agon_service                     (four endpoints)
  ├── POST /tournaments/:id/squads/auto-assign   preview, no writes
  ├── PUT  /tournaments/:id/squads/assignments   apply (bulk)
  ├── POST /matches/:id/sides/auto-assign        preview, no writes
  └── PUT  /matches/:id/sides/assignments        apply (bulk)
```

No new item types, no worker changes, and no new infrastructure. The
result is ordinary `squad` / `side_id` values, so standings, bracket fill,
rating, stats and the stream handlers don't change.

### 1. `team_builder::strength`: turn a player into an estimate

Input: the player's `UserRecord` (already holds `ratings.<ladder>` and
per-sport `stats` inline, fetched in one `Dao::batch_get_users`), the
match's sport tag, and the pool's group mean.

```rust
pub enum Signal { Rated, Blended, Placement, WinRate, New, Guest }

pub struct Estimate {
    pub mu: f64,          // what the solver balances on
    pub sigma: f64,       // what the solver spreads out
    pub signal: Signal,
    pub breakdown: Breakdown, // rating weight, raw/shrunk win rate, games: for the strength sheet
}
```

The rules, in order:

1. **Rating part.** Look up `ladder_for_tag(sport)` in `ratings`. That gives
   `μ_r`, `σ_r`, and `r = matches_rated`. Sport `other` has no ladder, so
   `r = 0`.
2. **Win-rate part.** Shrink toward 50% with
   `p = (wins + ½·draws + 3) / (played + 6)`, then map it onto the μ scale as
   `μ_w = μ_group + C_WR · logit(p)`. `σ_w` narrows with `played` but has
   a floor (`WR_SIGMA_FLOOR`) set wider than a settled rating, because win
   rate in a team sport is diluted by teammates.
3. **Blend.** `w = min(r, 10) / 10`. Then `μ = w·μ_r + (1−w)·μ_w` and
   `σ² = w·σ_r² + (1−w)·σ_w²`.
4. **No history** (`r = 0` and `played = 0`, or a guest with no `user_id`):
   `μ = μ_group`, `σ = INITIAL_SIGMA`.
5. **Organiser level** (if set): `Newer` puts `μ` at the group's 25th
   percentile and `Stronger` at its 75th. `σ` is left as is, so the player
   still counts as uncertain and still gets spread.

`μ_group` is the mean `μ` of the pool's players who have any history. If
nobody has any, it's `INITIAL_MU`. So there's a small two-pass step:
resolve the known players first, then the unknowns.

**Prerequisite:** win/draw/loss per sport is currently only exposed as
`matches_played_for_sport` in `agon_service::mapping`. Add a
`agon_core::sports::win_record(stats, tag) -> WinRecord { played, wins,
draws, losses }`, generated through `for_each_sport!` so a new sport can't
be missed. Then point the existing helper at it.

**Tunable constants** (`RATING_TRUST_MATCHES = 10`, `WR_PRIOR_GAMES = 3`,
`C_WR`, `WR_SIGMA_FLOOR`) live in one place. v1 ships with hand-picked
values. Calibrating them against real outcomes is a follow-up (see Later).

### 2. `team_builder::solver`: split the pool

```rust
pub struct PoolPlayer { pub id: String, pub est: Estimate, pub pinned_to: Option<usize> }
pub enum Method { Random, AutoBalanced }

pub fn build(players: &[PoolPlayer], caps: &[Option<u32>], method: Method, seed: u64)
    -> Result<Assignment, BuildError>;

pub struct Assignment {
    pub sides: Vec<Vec<String>>,
    pub balance: Balance,   // per-side Σμ, unsure count; overall verdict; 2-side win prob
}
```

- **Random:** place the pinned players, shuffle the rest with the seed,
  then deal round-robin into the sides that have room.
- **Auto Balanced:**
  1. Start from `snake_draft` (already in `agon_core::tournament`) over the
     unpinned players, sorted by `μ`.
  2. Hill-climb with pairwise swaps between sides (never touching pinned
     players) until a full pass finds no improvement.
  3. For 2 sides and n ≤ 16, solve exactly instead (≤ 12,870 splits).
     That's the common five-a-side / intra-squad case.
  4. The objective is lexicographic: first minimise the gap in `Σμ` (for 2
     sides, `|P(A beats B) − ½|` under the Weng-Lin model the engine already
     uses, `BETA` from `rating::engine`). Then, among splits within ε of the
     best, minimise the spread of `Σσ²`, which spreads out the unknowns.
  5. Re-roll: pick among the near-optimal splits using the seed, so each
     re-roll gives a different split that's just as fair.
- **Invariants** (all unit-tested): every player is placed exactly once,
  pins are respected, side sizes differ by at most 1 (except where pins
  force it), caps are never exceeded (`BuildError::OverCapacity` instead),
  and the same input + seed gives the same output.
- **RNG:** use `rand_chacha::ChaCha8Rng::seed_from_u64` (already in the
  lockfile) rather than `StdRng`, whose output isn't guaranteed stable
  across `rand` versions.

### 3. Preview / apply split

The preview computes and returns a proposal and **writes nothing**. Apply
is a separate bulk-assignment call that receives the **exact split from the
preview**, so it never re-runs the solver.

Why apply isn't "the same seed, but `apply: true`":

- What gets written is exactly what the organiser saw, even if a deploy
  changed the algorithm in between.
- Staleness is explicit rather than silent. The preview returns a
  `roster_version` (a hash of the pool's ids and current
  squads/sides), and apply sends it back. If anyone joined, left or was
  moved in the meantime, apply returns **409**, and the UI re-previews.
- The bulk endpoints are generic. A future drag-and-drop "save all" on
  the entries page can use them too.

#### Preview request

```jsonc
POST /tournaments/:id/squads/auto-assign      // and /matches/:id/sides/auto-assign
{
  "method": "random" | "auto_balanced",
  "scope": "unassigned" | "everyone",   // default "unassigned"
  "squad_count": 4,                     // tournament only; default ceil(pool / squad_size)
  "pinned": { "<entrant_or_player_id>": 2 },  // squad index or side_id
  "levels": { "<id>": "newer" | "stronger" }, // absent = as expected
  "seed": 12345                          // optional; server picks one and echoes it
}
```

With `scope: unassigned`, anyone already placed is treated as pinned
where they are.

#### Preview response (organiser only)

```jsonc
{
  "seed": 12345,
  "roster_version": "b64…",
  "verdict": "even" | "close" | "uneven",
  "spread": 6,                  // display points between squad averages (k sides)
  "win_probability": 0.51,      // 2 sides only, side order as listed
  "unsure_count": 5,
  "sides": [{
    "squad": 0,                 // or "side_id"
    "average": 1550,            // display scale, rounded to 10
    "unsure_count": 2,
    "players": [{
      "id": "…", "user_id": "…",
      "signal": "rated" | "blended" | "placement" | "win_rate" | "new" | "guest",
      "certainty": 1 | 2 | 3,
      "estimate": 1585,         // display scale; null when signal = placement
      "ranked_games": 7,        // null when signal = placement
      "win_rate": { "raw": 0.66, "adjusted": 0.61, "played": 12 },  // null if no games
      "rating_weight": 0.4,     // the strength sheet's bar
      "level": "newer" | "stronger" | null,
      "pinned": true
    }]
  }]
}
```

The `certainty` buckets come from `σ` (thresholds in `team_builder`), not
from game counts, so one rule drives both the dots and the solver.

#### Apply request

```jsonc
PUT /tournaments/:id/squads/assignments       // and /matches/:id/sides/assignments
{ "roster_version": "b64…",
  "assignments": [{ "id": "<entrant_or_player_id>", "squad": 2 }] }   // or "side_id"
→ 200 updated summary | 409 roster changed | 422 invalid (unknown id, over cap, …)
```

### 4. Guarding hidden ratings

The placement rule (`rating::published`) is enforced in **one** mapping
function: when the signal is `placement`, `estimate` and `ranked_games`
come back as `null`, and only the win-rate part is shown. The solver still
uses the hidden `μ`.

There's one residual leak: a side's `average` includes that hidden `μ`, so
an organiser could in principle subtract out an approximate value.
Mitigations: round averages to 10, keep the response organiser-only, and
compute the average after the level override. I'm treating that as
acceptable. If not, exclude placement players from the displayed average
(it's only cosmetic, and the solver is unaffected).

### 5. Bulk writes (DAO)

- **`Dao::reassign_match_players_tx(match_id, &[(updated, old_side)])`**:
  one `TransactWriteItems` with a `Put` per player plus **one** merged
  `headcount_delta_update`. It also drops `side_authority` grants for
  anyone moved off a side (in the same transaction, replicating the
  `SideAdmin`-revoke logic that `PATCH /matches/:id` runs per player
  today; extract that into a shared helper so the two paths can't
  drift). Up to 99 players fits one transaction, the same limit as
  `remove_match_players`. Call `refresh_side_roster_previews` once
  afterwards.
- **`Dao::set_tournament_entry_squads(tournament_id, &[(entrant_id, squad)])`**:
  conditional `Update`s (`attribute_exists`, `status = accepted`),
  chunked at 100 per transaction. A failed later chunk leaves a partly
  applied but still `open` tournament. Every entry stays editable, and
  re-applying the same assignment is idempotent, so the endpoint simply
  retries the remaining chunks. Fine for v1.

### 6. Service rules (shared by both surfaces)

- **Auth:** tournament `Owner`/`Admin` (`caller_tournament_authority_role`)
  or match `Owner`/`Admin` (`caller_match_authority_role`). Anyone else
  gets 403, the same people who can assign by hand today.
- **When:**
  - Tournaments: `status == "open"`, `entry_audience == Players`, and a team
    sport. Otherwise 409.
  - Matches: `status == "scheduled"` and no confirmed or pending score.
    Ranked is fine, since the ranked lock concerns the flag and score, not
    the roster.
- **Pool:**
  - Tournaments: entries with `status == "accepted"`.
  - Matches: players whose `occupies_slot()` is true. Pending, non-reserving
    invitees are left out. Players with no `user_id` are guests.
- **Caps:**
  - Tournaments: `ceil(pool / squad_count)` per squad.
  - Matches: each side's `max_players`.
  - Over capacity returns 422 with a message the sheet can show.
- **Labels:** unchanged. The server still invents no squad labels (per
  9b01d49). After apply, the entries page saves `defaultSquadLabel(i)` for
  any new squad, as it already does.
- **Telemetry:** a span per call with method, pool size, sides and solver
  time, plus a counter `team_builder_runs{method, surface, applied}`.

## Work breakdown

| # | Piece | Est. |
|---|---|---|
| 1 | `sports::win_record` + repoint `matches_played_for_sport` | ¼ d |
| 2 | `team_builder::strength` + unit tests (ramp, shrink, group mean, levels, placement → hidden) | 1 d |
| 3 | `team_builder::solver` + unit tests (invariants, determinism, exact vs brute force, unknowns spread, pins, caps, "balanced beats random" on fixtures) | 1½ d |
| 4 | DAO bulk writes, shared side-admin revoke helper, `roster_version` hash | 1 d |
| 5 | Four endpoints, request/response types, mapping (incl. placement guard), validation, telemetry, `make generate-schema` | 1½ d |
| 6 | `agon_tests` integration: preview/apply on both surfaces, 403, 409 stale `roster_version`, 409 wrong status, 422 over cap, placement fields null | 1 d |
| | **Total backend** | **~6 days** |

Pieces 1–3 are pure and can land on their own in a first PR with no API
change. Then 4–6. Ship tournaments before matches if we want to split it
further: the match side adds the side-admin and roster-preview details.

The frontend (entries page sheet, preview, strength sheet, match tray)
comes after that, on the generated client. Roughly another 3–4 days.

## Open items

- **Match board win-% range** ("could be 44–58%"): the drift band was
  dropped from tournaments. Should the range go from the match board too?
  (Leaning yes, for consistency.)
- **Initial constants** (`C_WR`, `WR_SIGMA_FLOOR`, certainty thresholds):
  I'll propose values in the first PR with a fixture table showing what
  they produce, for a gut check.

## Later (not v1)

- Per-round re-draft (`Auto { by }` formation) on top of `solver`.
- Keep-together / keep-apart pairs: a "reject this swap" constraint in the
  hill-climb.
- Positions (GK, bowlers): needs position data first. It would then be a
  per-side quota constraint.
- Calibration: replay rated history to check that "even" splits produced
  close games, and tune the constants.
- Creating new match sides on the fly ("split these 15 into three").
