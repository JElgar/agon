# Teams page: backend design

This is the backend for the teams design in `docs/teams-page-scope.md` and
the canvas. It covers what we can reuse, what's new, how each new piece
works, the hardest parts, and the infrastructure each piece needs.

## TL;DR

Most screens only need **thin endpoints over data we already store**. The
real work is one new pipeline: **team-scoped stats**. Today a match's stats
are rolled up per *user* (`StatContribution` → `USER#<id>.stats.<sport>`).
The team page needs the same roll-up per *team*, and per *team + player*,
sliced by competition kind, by tournament and by year. That pipeline is the
riskiest piece and the one everything else depends on.

None of this needs a new AWS service. It does need:

- a one-off backfill job (replay every existing match),
- a Meilisearch settings change and a full reindex,
- extra write load on the worker and on DynamoDB.

The worker and DynamoDB already handle the per-user version of this load.

| # | Workstream | Difficulty | Infra needed |
|---|---|---|---|
| B | Team-scoped stats aggregation (worker) | **Hard** | Medium: more worker and DynamoDB writes, no new services |
| G | Backfill / replay of existing matches | Medium | **Most**: a one-off job on the VPS, DynamoDB scan, reindex |
| F | Search index: `kind` + `tournament_id` filters | Easy | Medium: Meilisearch settings migration + full reindex |
| D | Team → tournaments edge + cached league position | Medium | Low: new item type, backfill entries |
| C | Read APIs (summary, leaderboard, competitions, teams tab) | Medium (N+1 care) | None |
| A | Match `kind` classification | Easy, but foundational | None |
| E | Team settings: sports, join default, resend invite | Easy | Low: backfill `sports` |
| H | Frontend-only items (nav, Matches tab, feed sort, guest tag) | Easy | None |

## What already exists (reuse, don't rebuild)

| Need | Existing piece |
|---|---|
| Per-match, per-user stat diffing that is idempotent, self-correcting and survives races | `Dao::reconcile_match_contribution` (`agon_core/src/dao/stats.rs`), driven by `agon_worker/src/handlers/stats.rs` on every match `#META` write |
| Async pipeline | DynamoDB stream → EventBridge Pipe → SQS (60s visibility, DLQ after 5) → self-hosted `agon_worker` on the OVH VPS |
| Sport-specific counters (goals, assists, runs, wickets…) | `agon_core::sports::contribution` |
| Sides tagged with a team | `MatchSideRecord.team_id` (can repeat across sides for an in-house game) |
| Tournament on a match | `MatchRecord.tournament_id`, `tournament_round` |
| League standings | `GET /tournaments/{id}/standings`, computed on read from `TMATCHES#<id>` (GSI1) |
| A team's matches, filtered and sorted | Meilisearch `matches` index: `team_ids`, `participant_ids`, `status`, `starts_at_ts`, `open`, `open_has_space` |
| My teams / members / roles / invite / transfer / delete | `/users/me/teams`, `/teams/:id/members*`, `/teams/:id/invitations`, `/transfer-ownership`, `DELETE /teams/:id` |
| Invitations inbox | `GET /users/me/invitations` (`UINV#<uid>` on GSI1) |
| Follow a team | `POST/DELETE /teams/:id/follow`; `Dao::list_followed_teams` exists but has **no endpoint** yet |
| Browse matches, "My games", "Open to ringers" | `BrowseMatchesPage` over `GET /matches` + `GET /feed`, unchanged |
| RSVP / "I'm in" / "9/11 confirmed" | `viewer_is_going`, `viewer_invitation_pending`, `team_join_enabled`, roster counts |

## A. Match kind (foundational, easy)

Every team stat and filter keys off one question: what kind of match is
this? Make it a pure function in `agon_core` and store the result.

```
kind(match, tournament) =
  league   if match.tournament_id and tournament.format = league
  cup      if match.tournament_id and tournament.format = knockout
  squad    if no tournament, ≥2 sides, and every side has the same team_id   (in-house)
  friendly if no tournament and ≥1 side has a team_id                       (vs another team or an ad-hoc side)
  pickup   otherwise                                                         (no teams at all)
```

- The kind is **match-level**. It's the same for every team in the match,
  so it can live on the match and in the search doc.
- **The setup sets it explicitly (decided 12 Nov).** The New match flow's
  Opposition choice writes `kind`:
  - Another team or One-off → `friendly`
  - In-house → `squad`
  - Tournament generation → `league` or `cup`

  The rules above are only the fallback, for matches written before the
  field existed and for the backfill. A team against an ad-hoc side counts
  as a friendly.
- Store it as `MatchRecord.kind`. Recompute it in the same write paths that
  set sides or the tournament (create, edit sides, tournament generation).
  The worker re-derives it defensively, so a stale value self-heals.

## B. Team-scoped stats aggregation (the hard part)

### What the UI reads

| Screen | Needs |
|---|---|
| Overview record (per sport) | P, W–D–L, win %, GF, GA, GD, clean sheets, over **league + cup + friendly** (`ext`) |
| Matches page (1b) record | The same, per kind: all-ext / league / cup / friendly, plus squad (games, goals, goals per game) |
| Friendlies page (3b) | Record per **year** and all-time, plus a leaderboard per year |
| Squad games page (3c) | Games, goals, goals per game per **year** and all-time, plus a leaderboard per year |
| Leaderboard (2) | Per player: apps, W, D, L, goals, assists… scoped to *all for team*, *a league season (tournament)*, *friendlies*. Career comes from the existing user stats |
| Top performers (overview), our top players (league page) | The same player rows, scope `all` / tournament |

### Item design (same table, `TEAM#<teamId>` partition)

Aggregates are summed counters:

```
TEAM#<t> / TSTAT#<sport>#<bucket>            team record, e.g. {played,won,drawn,lost,gf,ga,clean_sheets,games,goals}
TEAM#<t> / TPSTAT#<sport>#<bucket>#<userId>  team-player row, e.g. {apps,won,drawn,lost,counters{goals,assists,...}}
```

Contributions record what one match last added, mirroring the existing
`StatContribution`:

```
MATCH#<m> / TCONTRIB#<t>              {sport, buckets[], counters{}}  what this match last added to team t's record
MATCH#<m> / TPCONTRIB#<t>#<userId>    {sport, buckets[], counters{}}  what it last added to (t, user)
```

**Buckets** are the precomputed slices. For a match of kind `K`, starting in
year `Y`, in tournament `T`:

| Row | Buckets written |
|---|---|
| Team record, `K` ≠ squad | `ext`, `ext@Y`, `K`, `K@Y`, and `t:T` if there's a tournament |
| Team record, `K` = squad | `squad`, `squad@Y` (games and total goals only, with no W/D/L, per the in-house rule) |
| Team-player row | `all`, `K`, `K@Y`, and `t:T` if there's a tournament |

Squad games **are** included in player rows (in-house rule: leaderboard
includes them) and in `all`.

**Which players count:** everyone who occupied a slot on a side whose
`team_id = t`, member or not. That puts **ringers on the leaderboard**, as
decided. The read side flags non-members as guests (see C). In a squad game
both sides are team `t`, so each player is counted once, on their own side.
That falls out naturally.

### The reconcile algorithm (generalise what exists)

`reconcile_match_contribution` already does "diff against what this match
last contributed, apply the delta, and back out of the old sport if the
sport changed". The team version generalises **"sport changed" into "bucket
set changed"**:

```
stored  = get TCONTRIB / TPCONTRIB          (sport_old, buckets_old, counters_old)
desired = classify + score                  (sport_new, buckets_new, counters_new)
if equal → no-op                             (likes/comments fire #META writes constantly)
tx:
  put/delete contribution, conditioned on `stored` (optimistic lock, as today)
  for b in buckets_old − buckets_new: ADD −counters_old  on (sport_old, b)
  for b in buckets_new − buckets_old: ADD +counters_new  on (sport_new, b)
  for b in both (same sport):         ADD (new − old)    on b
```

This handles all of these:

- a re-score
- cancelling (back everything out)
- moving a match across New Year (`@2025` → `@2026`)
- attaching a friendly to a league
- changing a side's team
- changing the sport
- a squad game becoming a friendly

All of it uses one code path. As today, the operation is idempotent under
SQS redelivery and race-safe (a `Conflict` causes a redelivery).

**Per-sport team counters** are new. Each sport needs
`side_totals(score) -> {side_id: points}` (football goals, netball goals,
cricket runs, `Simple`/`Sets` points), plus optional extras: football
`clean_sheet = ga == 0`, cricket wickets taken. This is a small addition to
the `SportRecord` trait and its three modelled sports and two generic score
shapes.

**Max-type figures are left out of v1 (decided 12 Nov).** "Biggest win" or
cricket "highest total" can only ratchet up. If a 7–0 is later corrected to
3–0, the stored "biggest win" stays 7–0, because finding the next-biggest
means rereading every match. No designed screen shows one, so v1 has none.

### Where it runs

Add a new handler, `agon_worker/src/handlers/team_stats.rs`, and route it
after `stats::handle`. It is triggered by match `#META` writes, like the
existing stats handler. It must also run from the accept saga that calls
`reconcile_match_stats`, because a roster link doesn't touch `#META`.

### Write volume

Per confirmed match: 2 team-record transactions of ≤6 items, plus one
transaction per player of ≤5 items (≤9 when the bucket set moves). For an
11-a-side game that's ~24 transactions and ~130 item writes. On-demand
DynamoDB handles it, and it's the same order as today's per-user stats plus
ratings. Each per-team partition takes ≤~150 writes per match, far below
partition limits.

### Why it's the hardest part

- **Correctness under edits is the whole game.** Matches get re-scored,
  rosters change, sides change team, and a scheduled match moves date. Every
  combination has to back out cleanly, or totals drift forever with no
  cheap way to detect it.
- **Two dimensions now move**: buckets and sport. Today only sport can move.
- **Players can hold several roster entries.** The existing code takes the
  "best outcome"; the team version must use the entry on *this team's* side.
- **Year attribution needs a timezone.** A 00:30 kick-off on 1 Jan in London
  is still 31 Dec in UTC. The proposal is to use the tournament's
  `timezone`, else the match location's, else UTC.
- **Tests:** it needs a property-style test that applies a random sequence
  of match edits and checks the aggregates equal a from-scratch recompute.
  That test is what makes this safe.

**Alternative considered: compute on read** (the way standings work). It's
fine for one tournament (dozens of matches), but a team page summing
hundreds of matches and their box scores on every view is too slow and
costly. It also can't sort a leaderboard cheaply. We could use it as a
throwaway v0 for the current year only, but the materialised design is
what we should build.

## C. Read APIs

| Endpoint | Returns | Source |
|---|---|---|
| `GET /teams/:id/summary?sport=` | Record per bucket (`ext`, `league`, `cup`, `friendly`, `squad`, each all-time and per year on request); `form` (last 5 ext results); `streak`; `last10` (GF/GA); `next_match` | `TSTAT#` query (one partition, prefix); form/last10/next from Meilisearch `team_ids` + `kind != squad`, sorted by time |
| `GET /teams/:id/leaderboard?sport=&scope=all\|friendly\|squad\|t:<id>&year=&rank=contrib\|win` | Player rows with `is_member` (drives the **Guest** tag) and `is_me` | Query `TPSTAT#<sport>#<bucket>#`, then sort in the service. Bounded by roster plus guests, so reading the whole slice is fine. Hydrate names and avatars with `BatchGetItem` on profiles, and membership from `list_team_members`. Win rate: ≥5 apps |
| `GET /teams/:id/competitions` | Up to **4** boxes. Leagues and cups first (live first, with cached position), then Friendlies and Squad games if played. Includes a total count | Team → tournament edges (D) plus `TSTAT` for the friendly and squad summaries |
| `GET /matches` (extended) | New filters: `kind`, `tournament_id` | Search (F) |
| `GET /users/me/teams` (enriched) | Each team gets `role`, `form` (last 5), and `next_match` with your RSVP state | Two parallel search queries per team on the page (≤20). Or cache `form` on the `TSTAT#…#ext` row in the worker later if it shows up in latency |
| `GET /users/me/followed-teams` | Teams you follow, with a one-line status | Wraps the existing `Dao::list_followed_teams` |
| `GET /users/me/invitations` | Already exists. Check it carries team invites with crest data for the Teams tab | — |

**Career** on the leaderboard needs no new backend. It reads members'
`USER#<id>.stats` (members only, as decided).

## D. Team → tournaments edge + league position

A team has no lookup for its tournaments today. Entries live under
`TOURNAMENT#<id>/ENTRY#<team>`.

- **New item:** `TEAM#<t> / TOURN#<tournamentId>` holding `{name, format,
  status, position?, points?, played?}`. It's written in the **same
  transaction** as `create_tournament_entry`, and deleted with the entry.
- **Cached position:** when a league match's score is confirmed, the worker
  recomputes that league's standings (the same code as the endpoint, moved
  into `agon_core`). It writes `position` and `points` onto each entrant's
  edge item, so the overview's "2nd · 19 pts" is a single query.
- **Backfill:** scan existing `ENTRY#` items and write their edges.

## E. Team settings

- `TeamRecord.sports: Vec<String>` (ordered; first = default ★).
  - Accepted on create and on `PATCH /teams/:id`.
  - Returned on get.
  - **Backfill** from each team's match history: Meilisearch with a
    `team_ids` filter, facet on `sport`, ordered by count.
- `TeamRecord.members_join_default: bool`: the default for
  `MatchSide.team_join_enabled` when a match is created from the team.
- `POST /teams/:id/members/:member_id/resend-invite`: bump `invited_at`
  and re-send the notification. Rate-limit it (once per 24h).
- Squad page, roles, transfer, leave and delete already exist.

## F. Search index (Meilisearch)

- Add `kind` and `tournament_id` to the match document and to
  `Index::Matches.filterable_attributes()`.
- Update the existing settings-ensure on startup, then **reindex every
  match**. Done in the backfill job (G).
- This powers the Matches page (1b) filter, Friendlies and Squad games
  history, form/last 10 (exclude `kind = squad`), and the `kind` tag on
  Browse.

## G. Backfill / replay

All of B, D, E and F need existing data replayed. Because every reconcile
is idempotent and diff-based, **the backfill is just "run the handlers for
every match"**. The same job is the repair tool if aggregates ever drift,
and it's how we add a new bucket later.

- Add an `agon_worker backfill` subcommand that does the following:
  1. Paginated `Scan` (filter `type = match`, `SK = #META`).
  2. Per match: derive and store `kind`, upsert the search doc, then run
     `team_stats::reconcile`.
  3. Scan `ENTRY#` items and write the team → tournament edges.
  4. Backfill `Team.sports`.
- **Rate-limit** the job: concurrency 4–8, with backoff on throttling.
  - It shares the table with live traffic, and on-demand capacity can still
    throttle a burst.
- **Resumable:** log the scan's `LastEvaluatedKey` so it can resume.
- **Run it** as a one-off `docker run` of the worker image on the OVH VPS,
  with the same env as the worker. Run staging first.
- **Verify:** a `--verify` mode recomputes a sample of teams from scratch
  and diffs against the aggregates.

## H. Frontend-only (no backend)

- Bottom nav: Feed · Teams · ＋ · Matches · Profile.
- Matches tab = `BrowseMatchesPage`. Retitle it, drop the back arrow, and
  open it on Scheduled › My games.
- Feed **Coming up**: sort unanswered invites first, then joinable games,
  then games you're going to. Change "Browse all matches" to "See all ›".
- Guest tag from `is_member`.
- New match from a team: read `?team=` in `LogMatchPage` and limit sports
  to `Team.sports`.
- Delete the client-side 50-match approximation in `lib/teamStats.ts`. That
  also fixes the in-house random W/L bug.

## Hardest parts, ranked

1. **Team-scoped stats reconcile (B).** It's a generalisation of the
   trickiest code in the repo. It adds a second moving dimension (buckets),
   year and timezone attribution, and side-aware roster handling. Silent
   drift is the failure mode, so the property test matters most.
2. **Backfill and rollout (G).** It's the first time we replay the whole
   match history. It needs to:
   - be throttled and resumable,
   - run on the VPS outside the normal deploy,
   - be ordered correctly (deploy the worker with the new handler *before*
     the backfill, so live edits during the run converge).
3. **Leaderboard and teams-tab read performance (C).** Profile hydration and
   per-team search calls make N+1s that are easy to write. Batch and
   parallelise from day one.
4. **Cached league position (D).** It means moving standings into
   `agon_core` and keeping the cached copy in step. It's not hard, but it's
   one more place for drift.
5. **Per-sport side totals (B).** Each sport needs its own `side_totals`,
   cricket especially, and "max" figures need a decision.

## Infrastructure, ranked by how much each needs

1. **Backfill / reindex job.** This is the only real new operational piece.
   It's a new worker subcommand, a one-off container on the VPS, a full
   DynamoDB scan, and a Meilisearch reindex. There's no new AWS resource,
   but it needs a runbook and a staging dry-run.
2. **Worker and DynamoDB load.** Write volume per confirmed match goes up by
   roughly 2–3×, all within on-demand capacity.
   - Watch the worker's per-message time against the 60s SQS visibility
     timeout. Batch the per-player transactions with bounded concurrency.
   - Watch DLQ depth after launch, and add an alarm if there isn't one.
3. **Meilisearch.** Two new filterable attributes. A settings change
   triggers a re-index of that index; at our size that takes seconds to
   minutes.
4. **No new GSI, queue, Lambda or service.** Every new read is a
   PK + SK-prefix query on `TEAM#<t>`, or an existing search.

## Suggested build order

1. **Frontend-only (H)** plus team settings (E, without the backfill). Ships
   now.
2. **Match kind + search fields (A, F)**, with a reindex. This unlocks the
   Matches page filter, form, Friendlies and Squad games lists.
3. **Team → tournament edges (D)**, with a backfill. This unlocks the
   Competitions boxes.
4. **Team stats pipeline (B)** behind the property test, then **backfill
   (G)**, then the **read APIs (C)**. This unlocks the record, leaderboards
   and top performers.
5. Cached league position. Polish and drop the client approximations.

## Ranked games

Ranked is a **separate axis** from competition kind, and it already exists.

- `MatchRecord.ranked: bool` is chosen in the match setup's "How it counts"
  control, which today reads **Friendly | Ranked**.
- A ranked match moves each player's **per-sport rating** (Weng-Lin) once
  the score is agreed.
- **Teams are not rated**, by product decision (see the header of
  `agon_core/src/dao/rating.rs`).
- A ranked match **can't include players without an account**, and its
  ranked flag locks once a score is in or the match starts.
- League and cup matches are generated **unranked**. So are series
  occurrences.

How it fits the team pages:

| Question | Answer |
|---|---|
| Do team stats care about ranked? | **No.** Team records and leaderboards count ranked and unranked alike. Kind decides *where* a match counts; ranked decides whether it *also* moves ratings. No change to B. |
| Can a team match be ranked? | Yes, for anything created through New match (friendlies, squad games): the organiser chooses. League and cup generated matches are unranked today. |
| Should leagues be ranked? | A product call. To allow it, add `ranked` to `TournamentRecord` and copy it onto generated matches. It's small, but leagues with account-less squad members couldn't use it. |
| Where do ratings show? | Ratings are per player and per sport, already on profiles. The team leaderboard dropped its Rating view. A "Rating" column or tab from members' published ratings needs no new backend. |
| Squad games ranked? | They work well: balanced in-house sides are good rating data. |

**Naming clashes to fix before this ships:**

1. **"Friendly" means two things.** The live "How it counts" control uses
   it for *unranked*. The team pages use it for *a non-league game against
   another team*. In New match you could pick Opposition "Another team",
   then How it counts "Friendly", and get a ranked-looking choice with a
   misleading name. Recommendation: rename the toggle to **Casual |
   Ranked**, and keep "Friendly" for the team meaning, which is what it
   means in football.
2. **"Guest" means two things.** In the ranked rules it means a player with
   *no Agon account* (blocked from ranked, and with no stats at all). The
   leaderboard tag means a *non-member with an account* (a ringer). Players
   without an account never reach the leaderboard. Recommendation: either
   label the leaderboard tag **Ringer**, or rename the no-account case to
   "player without an account" in the ranked copy.

## Decided (12 Nov)

- A team against an ad-hoc side is a friendly, set explicitly in match
  setup (see A).
- Year boundary: tournament timezone → match location → UTC.
- No max-type figures in v1.
- Stats stay public for now. Later there will be a public team page and a
  members-only page.
