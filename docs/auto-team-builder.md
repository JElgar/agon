# Auto team builder — scope

Status: proposal (2026-11-10). Nothing here is built yet.

## The ask

When there's a pool of players and no fixed teams, let an organiser hit one
button and have the players split into sides. Two flavours:

- **Random**: shuffle and deal. Quick, fair, no judgement involved.
- **Balanced**: use rating and/or win rate so the sides come out as even as
  we can make them.

Two places this applies today:

1. **Teamless tournaments** (`entry_audience: Players` on a team sport,
   `team_formation: Manual`). Entrants join with `squad: None`, and the
   organiser currently assigns every one by hand via
   `POST /tournaments/:id/entries/:entrant_id/squad` before `start` will run.
2. **A single match with a shared pool**: an intra-squad match where both
   sides are the same team, or any match with `allow_unassigned` and a pile
   of unassigned players. Today the organiser drags them across one at a time
   (`PATCH /matches/:id` → `side_assignments`).

## What already exists (and what doesn't)

| Piece | Where | State |
|---|---|---|
| Snake draft over a seeded list | `agon_core::tournament::snake_draft` | Built and tested. Not called by anything. |
| Shuffle | `agon_core::tournament::random_seed_order` | Built, used for random seeding. |
| `TournamentTeamFormationRecord::Auto { by }` | `dao/records.rs` | Record exists. Its meaning is a **re-draft every round** (by `wins` / `rating` / `games_played`). `POST /tournaments` rejects it ("isn't supported yet"). |
| Per-entry squad assignment | `Dao::set_tournament_entry_squad` | Built, one entry per call. |
| Squad labels | `TournamentRecord::squad_labels` | Built. Always supplied by the client (`defaultSquadLabel`), and `start` rejects a squad with no label. |
| Side reassignment | `Dao::reassign_match_player_tx` | Built, one player per transaction. |
| Ratings | `USER#<uid>/#PROFILE` → `ratings.<ladder>` (`μ`, `σ`, `matches_rated`) | Built. Weng-Lin, per-sport ladders, team strength = sum of player `μ`. |
| Win/loss record | `UserStatsRecord` per sport (`matches_played`, `wins`, `draws`, `losses`) | Built. Counts friendlies as well as ranked. |

So most of the parts are already there. What's missing is the balancing
algorithm itself, one endpoint per surface, and bulk writes.

### Don't mix this up with `Auto` formation

`Auto { by }` means "build new squads every round from the tournament so
far". That's a bigger feature: it needs the worker to run a draft between
rounds, it changes rosters partway through a bracket, and `wins` has nothing
to go on in round 0. **What's proposed here is a one-off assistant**: it fills
in `squad` on the entries (or `side_id` on players) and stops there. After it
runs, the tournament is an ordinary `Manual` one and the organiser can tweak
any assignment before `start`. Once the algorithm below exists, a future
`Auto` mode can call it once per round, so this is a step toward that
feature without being blocked on it.

## Proposal

### 1. A pure module: `agon_core::team_builder`

This follows the same rule as `rating::engine` and `tournament`: no DAO, no
clock, no ids. Players and strengths go in, assignments come out.

```rust
pub struct PoolPlayer {
    pub id: String,              // entrant id (tournament) or player_id (match)
    pub strength: Strength,      // see "Strength signal" below
    pub locked_to: Option<usize>,// pinned to a side by the organiser
}

pub struct Strength { pub mu: f64, pub sigma: f64 }

pub enum Method { Random, Balanced }

pub struct BuildInput<'a> {
    pub players: &'a [PoolPlayer],
    pub side_count: usize,
    pub side_caps: &'a [Option<u32>], // MatchSideRecord::max_players, or squad_size
    pub method: Method,
    pub seed: u64,                     // caller supplies randomness, so tests are deterministic
}

pub struct BuildOutput {
    pub sides: Vec<Vec<String>>,
    pub balance: Balance,              // predicted win probabilities / spread
}
```

Invariants the tests pin down:

- Every player appears exactly once.
- Locked players stay where they were pinned.
- Side sizes differ by at most one, and no cap is exceeded. If the caps
  can't hold everyone, return an error rather than leaving players out.
- The same input with the same `seed` always gives the same output.

**Random**: shuffle with the seed, then deal round-robin into the sides with
room, after placing the locked players. Hitting "re-roll" just sends a new
seed.

**Balanced**:

1. Seed with `snake_draft` over players sorted by `μ`, desc (that's the
   existing function, and it's already a decent starting point).
2. Hill-climb with pairwise swaps between sides, skipping locked players.
   Accept any swap that lowers the objective, and stop when a full pass
   finds nothing. With realistic pools (≤ ~40 players) this runs in
   milliseconds.
3. For 2 sides and n ≤ 16, solve it exactly instead (C(16,8) = 12,870
   splits, which is trivial). That's the most common case (five-a-side,
   intra-squad), and an exact answer makes the tests easy to write.
4. Re-roll for balanced: gather every split within ε of the best objective
   and pick one using the seed. The organiser gets a fresh but equally fair
   split instead of the same one every time.

**Objective**: use the Weng-Lin model the rating engine already uses.
Team strength is `Σμ` and team variance is `Σσ² + n·β²`. For 2 sides,
minimise `|P(A beats B) − 0.5|`. You can take that from skillratings'
Weng-Lin expected-score helper, or compute the same closed form inline with
`BETA` from `rating::engine`. For k sides, minimise the spread
(max − min) of `Σμ`, and break ties on the variance of `Σμ`.

Using the engine's own model buys us one thing: "balanced" means exactly
what the rating system would call a 50/50 game. Nobody's rating moves
because the teams were lopsided. If the split were balanced on some other
measure, the rating engine could still treat the game as uneven.

Uneven side sizes (an odd pool) are handled for free. `Σμ` already counts
the extra player, so the optimiser compensates by putting stronger players
on the short side.

### 2. Strength signal: rating, win rate, or both

The organiser's pick is **Balance by: Rating | Win rate | Auto** (the
default). Every option comes down to a `Strength { mu, sigma }` per player,
so the algorithm only needs to understand one thing.

| Player has… | `rating` | `win_rate` | `auto` (default) |
|---|---|---|---|
| A rating on this sport's ladder | stored `μ, σ` | win rate → `μ` | stored `μ, σ` |
| No rating, but stats on this sport | prior `INITIAL_MU, INITIAL_SIGMA` | win rate → `μ` | win rate → `μ`, wide `σ` |
| Nothing (new user, ad-hoc guest with no `user_id`) | prior | prior | prior |

- **Win rate → μ**: shrink it toward 50% first,
  `p = (wins + ½·draws + k) / (played + 2k)` with `k ≈ 3`. Without that, a
  player who is 1-for-1 looks unbeatable. Then map it onto the μ scale as
  `μ = INITIAL_MU + c·logit(p)`. Set `σ` from `played`, so a long record
  counts for more than a short one.
- Win rate counts friendlies and every opponent the same. It's noisier
  than the rating, so `auto` only uses it for players without a rating.
  That fills the gap for the many players who haven't played a ranked game
  in this sport yet.
- Look the ladder up with `rating::ladder_for_tag(match_type)`. Sport
  `other` has no ladder, so it falls back to win rate, then the prior.
- **Use μ, not the displayed rating**: the balancer reads the raw stored
  `μ`. That includes players still in placement
  (`matches_rated < PLACEMENT_MATCHES`). Their number isn't shown yet,
  but it's still the best guess we have.

### 3. Don't leak hidden ratings

Ratings in placement are deliberately hidden (`rating::published`). The
balancer may use them, but its response must not show them:

- Return per-side **balance** only: predicted win % for two sides, or a
  "spread" bar for more.
- Don't return per-player strength, and don't return per-side `Σμ` in
  rating units. Display ratings for published players already appear on
  their profiles, so the UI can show those next to names if we want.
- Say *which signal* was used per side ("8 rated, 2 by win rate, 1 new")
  so the organiser knows how much to trust the split. Don't say which
  player was which.

### 4. API

Both endpoints share one input shape. Each returns a proposal and can apply
it in the same call.

```
POST /tournaments/:tournament_id/squads/auto-assign
POST /matches/:match_id/sides/auto-assign

{
  "method": "random" | "balanced",
  "balance_by": "auto" | "rating" | "win_rate",   // balanced only
  "scope": "unassigned" | "everyone",            // fill gaps vs reshuffle all
  "squad_count": 4,                              // tournament only; else derived from squad_size
  "locked": ["<id>", ...],                       // keep these where they are
  "seed": 12345,                                 // optional; server picks one if absent and echoes it
  "apply": false                                 // false = preview, true = write
}
→ { "seed", "sides": [{ "squad" | "side_id", "member_ids": [...] }], "balance": {...}, "signals": {...} }
```

- **Preview, then apply**: the UI calls with `apply: false` to show the
  split, re-rolls by changing `seed`, then calls again with the same seed
  and `apply: true`. The algorithm is deterministic, so the applied split
  is the one that was previewed, as long as the roster hasn't changed in
  between. If it has, the result is recomputed and returned; it isn't
  silently different.
- **`scope: unassigned`** (the default) only places people with no
  squad/side and treats everyone already placed as locked. That's what you
  want when stragglers join after the first split.
- **Permissions**: tournament `Owner`/`Admin` (`TournamentRecord::authority`),
  or match `Owner`/`Admin` (`caller_match_authority_role`). The same people
  who can already assign by hand.
- **When it's allowed**: tournament `status == "open"`, and the match is
  `scheduled` with no score. Assigning to a ranked match is fine. The
  ranked lock is about the `ranked` flag and score, not the roster.
- **Labels**: the server still invents no squad labels (per 9b01d49).
  After applying, the entries page saves `defaultSquadLabel(i)` for any
  newly used squad, the same way it already does when the first entrant
  lands in one. The other option is to accept an optional `labels` array in
  the request. Recommend the client-side path, since it's what the page
  already does.

### 5. Writes

The writes today are one item at a time. A 20-person auto-assign shouldn't
be 20 separate transactions that can partly fail.

- **Matches**: add `Dao::reassign_match_players_tx(match_id, &[(updated, old_side)])`.
  It does one `TransactWriteItems` with a `Put` per player plus **one**
  merged `headcount_delta_update` for all the side deltas. 99 players fit
  in one transaction, which covers every real roster (same caveat as
  `remove_match_players`). Then call `refresh_side_roster_previews` once.
- **Tournaments**: add `Dao::set_tournament_entry_squads(tournament_id, &[(entrant_id, squad)])`.
  It does conditional `Update`s (`attribute_exists`, entry `accepted`) in
  transactions of up to 100, chunked. If a chunk fails partway, the
  tournament is still `open` and every entry is still editable by hand, so
  the endpoint can simply be run again (with `scope: everyone` and the
  same seed it produces the same result).
- **No new item types**: the result is ordinary `squad`/`side_id` values,
  so standings, bracket fill (`fill_tournament_bracket_squad_side`),
  rating, stats and the worker don't change.

### 6. UI

- **Tournament entries page**: an "Auto-assign" button next to the squad
  columns. It opens a sheet with Random / Balanced, a squad count, and
  "only unassigned" vs "reshuffle everyone". The preview shows the squads
  with a balance meter and a **Re-roll** button, then **Apply**. A pin icon
  on an entrant locks them before re-rolling.
- **Match edit page / unassigned tray**: the same sheet, minus the squad
  count (the sides already exist). The obvious place is next to the
  unassigned list on an intra-squad match.

## Out of scope for v1 (worth noting)

- **Per-round re-draft** (`Auto { by }` formation): see above. It can be
  built on this later.
- **Keep-together / keep-apart** (mates who want to play together, rivals
  to split up): easy to add as constraints in the swap step later, since
  it's just "reject a swap that breaks a pair".
- **Positions** (goalkeepers, bowlers): we have no per-player position
  data, so there's nothing to balance on. If it's added later, it becomes
  a per-side quota constraint in the same swap loop.
- **Learning from results**: checking whether "balanced" splits actually
  produced close games. The rating history already holds the data (the
  contributions keep `before` ratings), so this could be an analysis job
  later.
- **Generating sides that don't exist**: the match endpoint fills existing
  sides. Creating sides on the fly (e.g. "split these 15 into three") is a
  small extension, but it touches side creation, names and colours.

## Rough plan

1. `agon_core::team_builder`: random + balanced + tests (exactness against
   brute force for small n, invariants, determinism by seed, "balanced beats
   random" on fixtures). **~1 day.**
2. Strength resolution (rating → win-rate fallback → prior) as a small
   function over `UserProfileRecord`, with a batch profile read for the
   pool. **~½ day.**
3. Bulk DAO writes (`reassign_match_players_tx`, `set_tournament_entry_squads`).
   **~½ day.**
4. Two endpoints, regenerate the schema, integration tests in `agon_tests`.
   **~1 day.**
5. UI sheet on the tournament entries page, then the match edit page.
   **~1–1½ days.**

Shipping order: tournament first (it's the real pain today, where every
entrant is assigned by hand before `start`), then matches.

## Open questions

1. Should `auto` blend win rate in for *rated* players who are still in
   placement, or trust `μ` once there's at least one rated match?
   (Proposal: trust `μ`; its `σ` already says how unsure it is.)
2. Should the preview show published display ratings beside names, or keep
   it to the balance meter only?
3. For a teamless tournament, should `squad_count` default to
   `entry_count / squad_size` (as `snake_draft`'s doc assumes), or should
   the organiser always pick it?
4. Should players be able to see that their squads were auto-balanced
   (e.g. a badge on the tournament), or is that organiser-only detail?
