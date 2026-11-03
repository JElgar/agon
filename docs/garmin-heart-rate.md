# Garmin heart rate on matches

Goal: pull heart-rate data from the Garmin activity a match's players are
already recording, and attach it to the match. Options considered, chosen
approach, and what's built so far vs. left as follow-up.

## Options considered

1. **Live stream from the Agon Connect IQ watch app itself.** The watch app
   already reads heart rate (`ActivityStatsView.mc`, `Activity.
   getActivityInfo()`) and already has an authenticated device channel to the
   API — the pairing/device-token system built for live scoring (see
   `docs/garmin-live-scoring.md`). **Chosen** — cheapest to add, reuses
   existing infrastructure. Limitation: only covers players who've paired
   their own watch with the Agon app and have it open during the match, not
   the full lineup.
2. **Garmin Health API (official partner webhook push).** Garmin pushes a
   finished activity (including HR) to a webhook for any user who's granted
   OAuth consent via Garmin Connect. The only route that could cover every
   player automatically, but requires applying to and being approved for
   Garmin's Connect Developer Program as a business — real approval risk and
   delay, not worth it at this scale. Also post-match only, never live.
   Rejected.
3. **Manual FIT file import.** A player exports their Garmin Connect
   activity and uploads it; the server parses HR out of the FIT file. No new
   auth, no Garmin approval, works for any player on any device. Not chosen
   for now, but a reasonable fallback if "every player, not just whoever's
   running the Agon watch app" ever becomes a hard requirement.

## Design

Raw samples and their derived summary are both scoped to one metric, one
user, one match — generalized beyond heart rate from the start, since GPS
(for heat maps), distance, cadence etc. are the obvious next additions:

- **Sampling/upload cadence**: the watch samples HR every 5s (matching the
  poll cadence `agonView`/`ActivityStatsView` already use) and flushes a
  batch of 6 samples every 30s.
- **Raw time series** — `Pk::Match(mid)` / `Sk::MetricBatch { metric,
  user_id, started_at }` → `METRIC#<metric>#<userId>#<startedAt>`
  (`agon_core/src/dao/keys.rs`). `metric` leads so every user's batches for
  one metric list together in one query (`Sk::metric_prefix`, e.g. every
  player's GPS points for a heat map); `user_id` next so one user's own
  trace is also a single prefix (`Sk::metric_player_prefix`, e.g. one
  player's HR graph). Keyed on `user_id` (not a match-specific roster
  `player_id`) so it lines up with `StatContributionRecord`'s own key, and
  so a device can only ever attribute samples to its own paired account —
  no caller-supplied id, no impersonation surface. Appended via
  `POST /matches/:match_id/metrics/:metric/batches` (device-scoped,
  `SCOPE_LIVE_SCORING` — same paired-device identity as live scoring, but no
  match-admin check: posting your own heart rate isn't an organiser action).
- **Per-player derived stats**: a new top-level `heart_rate` field on the
  existing `StatContributionRecord` (`STATCONTRIB#<uid>`), not a new item
  type — a roster view rendering N players' stats (goals, assists, and
  whatever gets added later) stays one `Query` regardless of how many stat
  types exist. `heart_rate` stores `{max_bpm, sum_bpm, sample_count}` (avg
  derived at read time) and is updated incrementally (`Dao::
  append_metric_batch` → `apply_heart_rate_batch`): an unconditional `ADD`
  on `sum_bpm`/`sample_count`, and a best-effort conditional `SET` on
  `max_bpm` that only applies when the batch actually beats the stored max.
  This is deliberately *not* folded into `Dao::reconcile_match_contribution`
  (which fully recomputes `counters` from the match's confirmed score on
  every match-meta write): there's no "current state" to recompute a live
  wearable stream from, only an accumulating total, so the reconciler
  instead just carries `heart_rate` forward unchanged.
- **A real correctness wrinkle this surfaced**: the reconciler normally
  *deletes* a `StatContributionRecord` outright whenever `played == 0` (no
  confirmed-score outcome yet). HR accumulates throughout a live,
  unconfirmed match — and the reconciler fires on *any* match-meta write,
  including an unrelated like/comment on a still-in-progress match. Without
  a fix, the first comment on a live match would delete the in-progress HR
  data. Fixed: the reconciler now keeps the item alive (as a `Put`, not a
  `Delete`) whenever `heart_rate` is present, even with `played == 0`. See
  `Dao::reconcile_match_contribution`'s doc comment.
- **Not summarizable as a scalar** (a GPS heat map is the track itself, not
  a max/avg) stays read straight from the raw `METRIC#gps#` batches, same as
  a player's own HR graph reads `METRIC#heart_rate#` rather than the summary
  item.

## What's built so far

Backend: the generalized `MetricBatch` key, `MetricBatchRecord`/
`HeartRateContributionRecord`, `Dao::append_metric_batch`/
`list_metric_batches`, the reconciler fix above, and
`POST /matches/:match_id/metrics/:metric/batches` (heart rate only for
now — any other `metric` is rejected with a validation error until it gets
its own payload shape and, where it needs one, its own derived summary).
Unit-tested (key round-trips, partial-item deserialization, the
heart-rate-only-contribution shape) — not yet exercised against a real
DynamoDB table (this sandbox has none wired up) or `monkeybrains.jar`.

## What's left to build

1. **Watch-app wiring**: actually sample HR every 5s and flush batches every
   30s to the new endpoint, reusing `LiveApiClient`'s request-queue pattern.
   Not yet started — separate PR.
2. **A read endpoint** for a player's HR trace/summary (for an HR graph, and
   for the roster view's "Xbpm avg / Ybpm max" line) — `list_metric_batches`
   exists in the DAO but isn't wired to an API route yet.
3. **GPS** (or any other metric): a new `metric` value, its own sample
   payload shape (lat/lng, not a scalar), and — if it needs one — its own
   derived-summary field alongside `heart_rate`. The key/DAO shape already
   supports this; only the per-metric payload and summary are new work.
