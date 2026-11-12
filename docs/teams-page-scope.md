# Teams page: scope

Status: **design exploration**. There's a fixture-only prototype at
`/design/team` (dev builds only, no login or backend needed). See "Running the
prototype" at the bottom.

## What exists today

**Reaching it.** `/teams` ("My teams") and `/teams/:teamId`. On mobile neither
is in the bottom nav (Feed · + · Profile) or the top bar (Search, Notifications).
The only way in is the Account section of the profile page, or a team name on a
match card. On desktop it's a sidebar item.

**`/teams` list** (`TeamsPage`, `TeamCard`): crest, name, follower count. No
role or member count, because `TeamListItem` doesn't carry them.

**`/teams/:teamId`** (`TeamPage`) has three blocks:

| Block | Source | Limits |
|---|---|---|
| Header: crest, name, followers, member count, edit/invite/delete/leave/follow | `GET /teams/{id}`, `GET /teams/{id}/members` | Member count is "what's been fetched so far". `Team` has no total. |
| Team stats: matches/wins/win-rate banner, plus a per-sport win-rate bar | `lib/teamStats.ts`, computed **client-side** from `GET /matches?team_id=&limit=50` | Only counts the latest 50 matches. W/L only, so no draws in the headline and no goals or runs. |
| Members list with role management | `GET /teams/{id}/members` | Nothing about how anyone plays. |
| Recent matches (5) | `GET /matches?team_id=&to=now` | |

**Backend facts that shape the design:**

- `Team` is `{id, name, logo, invite_token, follower_count, is_followed_by_me}`.
  It has no sport, colour, founded date or stats.
- A team appears in a match only as `MatchSide.team_id`. The players on a side
  are that match's roster, not the team's membership list.
- Per-player stats exist, but only as **lifetime totals across all matches**
  (`UserProfile.stats`). There's `GenericPlayerStats` for W/D/L, and rich
  counters for football (goals, assists), cricket (runs, wickets, average,
  economy, catches…) and netball (goals).
- The worker builds those totals from one `StatContribution` item per
  (match, user), at `MATCH#<id>` / `STATCONTRIB#<user>`. It diffs and
  reconciles them on every match write (`agon_worker/src/handlers/stats.rs`,
  `Dao::reconcile_match_contribution`).
- Ratings are per ladder (= sport) and public once published (`LadderRating`).
  A player who is still in placements must not be shown a number.

## What the prototype proposes

The page gets a header, then an always-visible **form strip**, then tabs:
**Overview · Leaderboard · Matches · Squad**. Teams that play more than one
sport also get a **sport switcher**, which scopes every section. A cricket
record and a football record don't add up into anything meaningful.

1. **Header.** Adds sport pills and "since" (the team's created date). The
   primary CTA becomes **Log match**, pre-filled with this team as a side.
   Invite stays.
2. **Form strip.** Last 5 results as W/D/L tiles, plus the current streak if
   it's 2 or more.
3. **Overview**
   - **Record banner.** Played, W–D–L, win rate, a proportional W/D/L bar, and
     sport-specific extras. Football: goals for/against, goal difference,
     clean sheets. Cricket: runs, wickets, highest total, run rate.
   - **Top performers.** A dark spotlight card: top scorer, most assists (or
     runs/wickets for cricket), highest rated.
   - **Leaderboard preview.** Top 5 with "See all".
   - **Next up.** The next fixture with "9/11 confirmed" and an "I'm in"
     button.
   - **Last 10.** A mirrored bar chart of scored vs conceded, with result dots.
   - **Records.** Biggest win, longest win streak.
   - **Rivals.** Most-played opponents, with head-to-head W–D–L.
4. **Leaderboard**
   - **Metric chips.** Football: Rating, Goals, Assists, G+A, Apps, Win%,
     G/match. Cricket: Rating, Runs, Wickets, Avg, Economy (lower is better),
     Catches, Apps.
   - **Podium** for the top 3, then a ranked table. Each row has a movement
     arrow (places moved since the previous match), a "You" highlight, an
     owner crown, and a context column.
   - **Scope toggle.** *For this team* (only matches played on this team's
     side) or *Career* (lifetime stats, which exist today).
   - **Period toggle.** *This season* or *All time*.
   - **Guardrails.** Rate metrics only rank players with 5+ appearances.
     The Rating board leaves out players still in placements and says so.
   - **Badges.** Hat-trick, Ever-present, 5-for, and so on.
5. **Matches / Squad.** These are today's lists, unchanged in substance.

## What it needs from the backend

The prototype's fixtures (`agon_ui/src/pages/design/teamFixtures.ts`) are
shaped as these responses.

### A. Team-scoped player stats (the leaderboard). New, and the core of this work.

Lifetime stats can't answer "Marcus's goals *for Hackney Wick*". Proposal:

- When the stats reconciler applies a (match, user) contribution, and that
  user's side has a `team_id`, also apply the same counter delta to
  `TEAM#<team>` / `PLAYERSTATS#<sport>#<user>`. Record the `team_id` on the
  `StatContribution` so that a side's team changing, or a player moving
  sides, backs the delta out of the right place. The reconciler is already
  diff-based and idempotent, so this mostly reuses existing machinery.
- `GET /teams/{id}/leaderboard?sport=` returns every row, and the client
  sorts by metric. That is fine at squad scale: one `Query` on the
  partition, with no GSI per metric.
- Rating stays on the player (`LadderRating`). Ratings are per sport, not per
  team, so the "Rating" board means "this squad ranked by their published
  ratings".
- **Backfill.** A one-off job replays existing confirmed matches through the
  reconciler.

### B. Team record and form

- `TEAM#<team>` / `TEAMSTATS#<sport>` with played/W/D/L and the sport's
  for/against counters (goals, runs, wickets), maintained in the same
  reconcile pass, once per side rather than per player. This replaces the
  client-side 50-match approximation in `lib/teamStats.ts`.
- Form, streaks, biggest win and the Last-10 chart can come from the
  existing `GET /matches?team_id=` (latest 10, client-side) at first.
  Longest streak and biggest win over the team's whole history would need to
  be stored on the stats item. Defer that.

### C. Smaller additions

- `Team.member_count` and `TeamListItem.member_count` / `my_role`. These fix
  the "18+" count and the list row the redesign wanted.
- `Team.sports`, either derived from `TEAMSTATS#*` or set explicitly. This
  drives the pills and the switcher.
- **Rivals.** Group the team's matches by the opposing `team_id`. Do it
  client-side over recent matches first, or store per-opponent counters if
  it proves popular.
- **Movement arrows** need the previous ranking: snapshot rank on each
  reconcile, or diff against the leaderboard as of the previous match.
  This is a nice-to-have. Cut it first.
- **Season** needs a definition. Calendar year? A per-team season start date?
  Open question, see below.

## Suggested phasing

1. **Discoverability.** Ships independently, no backend. Put Teams on mobile:
   either a 4th bottom-nav stop, or a "Your teams" row of crests at the top
   of Profile and on the Feed. Show role and member count in the list once
   C lands.
2. **Leaderboard on lifetime stats.** No new backend. Fetch each member's
   `GET /users/{id}` (squad-sized, so N requests is acceptable for a first
   cut) and rank by career stats. That's the "Career" toggle. It validates
   the UI before investing in A.
3. **Team-scoped stats (A + B).** Worker aggregation, two endpoints,
   backfill. Switch the leaderboard's default to "For this team" and replace
   `lib/teamStats.ts`.
4. **Overview polish.** Next fixture, Last-10 chart, records, rivals,
   badges.

## Open questions

- **What is a "season"?** Calendar year, rolling 12 months, or
  owner-defined? This affects storage: per-season items are keyed
  `PLAYERSTATS#<season>#<sport>#<user>`.
- **Ringers.** A non-member who played for the team's side: do they appear
  on the team leaderboard? The prototype assumes yes, if they played on the
  side, perhaps with a "guest" tag. The alternative is members only.
- **Privacy.** Should non-members see the leaderboard, or only the record?
  Today everything on the team page is public.
- **One sport per team, or many?** Clubs often play several. The prototype
  supports many, with a switcher.
- **Badges.** Which ones, and are they computed (from bests or counters) or
  awarded by admins?

## Running the prototype

```bash
cd agon_ui && npm ci
```

```bash
VITE_SUPABASE_URL=http://localhost:8000 VITE_SUPABASE_ANON_KEY=local-dev-anon-key npm run dev
```

Then open <http://localhost:5173/design/team>. The Supabase values are
placeholders. The route bypasses auth and makes no network calls. In
`App.tsx` it's behind `import.meta.env.DEV` and lazy-loaded, so it isn't in
production builds.
