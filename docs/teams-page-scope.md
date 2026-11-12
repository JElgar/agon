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
**Overview · Leaderboard · Matches**. Teams that play more than one
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
5. **Matches.** Today's list, unchanged in substance.
6. **Squad is not a tab.** It's its own page, opened from the member count in
   the header (anyone) or Team settings → Members & roles. It has search,
   groups Owner / Admins / Members / Invited, and today's role and remove
   actions for admins.

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
- **`Team.sports: Vec<MatchType>`**, set explicitly. Teams have no sport
  today: create takes logo, name and invitees, and edit takes logo and name.
  - Chosen in **Create a team** ("Sports you play"; the first one picked is
    the default).
  - Edited in a new **Team settings** page, which replaces the Edit dialog.
    Settings covers profile, sports, joining (invite link and a team-wide
    "members can join matches" default), members & roles, and the danger
    zone (transfer, delete).
  - It drives the header pills, the sport switcher, and New match's sport
    step. With one sport, that step is skipped.
  - Backfill existing teams from the sports in their match history.
- **Movement arrows** need the previous ranking: snapshot rank on each
  reconcile, or diff against the leaderboard as of the previous match.
  This is a nice-to-have. Cut it first.
- **Season** needs a definition. Calendar year? A per-team season start date?
  Open question, see below.

## In-house games (a team playing itself)

A match can have more than one side linked to the same `team_id`. The
`MatchSide` docs already allow it ("a derby", "1st XI / 2nd XI"). For a
kickabout squad, Bibs vs Non-bibs is often *most* of what it plays.

**Classify each team match:** *external* if at least one side is not this
team; *in-house* if every side is this team.

| | External | In-house |
|---|---|---|
| Team record, form, streaks, win rate | ✓ | ✗ (the team would get a W **and** an L, so win rate drifts to 50%) |
| Goals/runs for and against, clean sheets, biggest win | ✓ | ✗ (every goal counts both for and against) |
| Player leaderboard | ✓ | ✓, on by default, with a filter: All · vs other teams · In-house |
| Squad-game extras: side-name tally, best partnerships | n/a | ✓ |

A team with no external games leads its overview with "Squad games", not
an empty record.

**Bug today.** `teamSide()` in `agon_ui/src/lib/teamStats.ts` uses
`sides.find(side => side.team_id === teamId)`, which returns whichever side
is listed first. So an in-house game counts as an arbitrary win or loss in
the current team stats banner. Small fix: skip matches where more than one
side has this team's id.

**Backend.** In the aggregation (A/B), the team-record contribution is
skipped for an in-house match. Per-player team stats are still applied but
kept in separate counters (external vs in-house), so the leaderboard filter
is a choice of counter, not a re-query. Partnerships need pairwise counts. Compute them client-side over the in-house match list
first. Store them only if they prove popular.

## Friendlies: two kinds

Matches outside any league or cup come in two kinds. Each gets its own box in
the team's Competitions list, after any leagues and cups. There are at most 3
boxes, and a friendly box only appears if the team has played that kind of
game.

- **Friendlies (vs other teams).** Results matter. The page shows the win
  rate (W–D–L), the match history, and a leaderboard (the Leaderboard's
  Counting dropdown gains a "Friendlies" option).
- **Squad games (in-house).** Both sides are this team, so the sides mean
  nothing: no Bibs vs Non-bibs. Keep it simple:
  - games, goals, and goals per game
  - top contributors (most goals, most assists, most games)
  - match history with scores and scorers; each row opens the match

Both come from the team's matches that have no `tournament`, split by whether
the other side is this team. A first cut needs no new backend.

## Leagues

Leagues already exist: tournaments with `format: league`,
`entry_audience: teams`, `GET /tournaments/{id}/standings` (P/W/D/L/points)
and `GET /tournaments/{id}/matches` (matchday per match). Generated matches
also carry `Match.tournament`. So this isn't a choice between building
leagues with teams or later. Build the team page first and treat a league as
a **filter on it**:

- **Competitions card** on the Overview: each tournament the team is entered
  in, with its current position or round.
- **Team-in-a-league view**: the standings with our row highlighted, position
  by matchday, remaining fixtures, and our top players in that league only.
- **Leaderboard scope** gains "this league" alongside "all for this team" and
  "career".
- **"Season" = a league.** This answers the open question below without
  inventing a season concept.

Gaps:

- **No team → tournaments lookup.** Entries live at
  `TOURNAMENT#<id>` / `ENTRY#<team>`. Add a reverse `TEAM#<team>` /
  `TOURNAMENT#<id>` edge item, written in the same transaction as the entry.
  Or, for a first cut, derive it from the `tournament` field on the team's
  matches.
- **Per-league player stats.** Use the same aggregation as A, keyed
  additionally by tournament id when the match has one.
- **Position by matchday.** Replay standings over the confirmed matches in
  round order, client-side. That's fine at league sizes.

## Suggested phasing

1. **Discoverability.** Ships independently, no backend. **Decided:** Teams
   gets its own tab in the mobile bottom nav. Nothing team-related goes in
   the feed: no "Your teams" row and no "need N more" nudges. Fixture
   call-ups live on the team page's *Next up* card. Show role and member
   count in the list once C lands.
2. **Leaderboard on lifetime stats.** No new backend. Fetch each member's
   `GET /users/{id}` (squad-sized, so N requests is acceptable for a first
   cut) and rank by career stats. That's the "Career" toggle. It validates
   the UI before investing in A.
3. **Team-scoped stats (A + B).** Worker aggregation, two endpoints,
   backfill. Switch the leaderboard's default to "For this team" and replace
   `lib/teamStats.ts`.
4. **Overview polish.** Next fixture, Last-10 chart, records.

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

## Ideas backlog

Not in v1. Parked here so they aren't lost.

- **Rivals.** The most-played opponent teams, with head-to-head W–D–L.
  Removed from the overview on 12 Nov.
  - *Definition:* a rival is another team we've met at least twice. The
    opponent side must have a `team_id` that isn't ours. Ad-hoc opponents
    have no stable identity and are excluded, and so are in-house games.
    Rank by meetings, with the most recent breaking ties. Show the top 3.
  - *Build:* group `GET /matches?team_id=` results by the opponent's
    `team_id`, client-side first. Later, add per-opponent counter items in
    the stats reconciler for full history.
  - *Extras:* a "Nemesis" (most losses against) and a "Favourite opponent"
    (most wins against).
- **Season Wrapped.** A recap in the style of Spotify Wrapped when a league
  season ends. Badges were removed from the leaderboard on 12 Nov and
  belong here instead.
  - *What:* a swipeable recap for each player: their goals and assists, best
    game, win %, who they played with most, and the team's finish. It also
    includes a "Your nemesis" slide: the teammate who beat you most in
    in-house games ("Amira beat you 7 times"), with your best partner
    alongside. This is the only place head-to-heads between teammates
    appear. The in-house view has no "You vs the squad" card.
  - *Badges:* awarded at the end of the season and kept on the player's
    profile, tagged with the season they're from (e.g. "Top scorer · ELSL
    S4"). Examples: Top scorer, Playmaker, Ever-present, Hat-trick, Clean
    sheets.
  - *Build:* triggered when the tournament status becomes `completed`.
    Badges are computed from that season's team-scoped stats and bests,
    not handed out by admins. It depends on the per-league player stats
    from A.

- **Home vs away.** Tag fixtures as Home or Away and split the team record
  by venue. Removed from the league fixtures on 12 Nov. Nothing in the data
  model says which side is at home today. It would need a home side on
  matches, or a home ground on teams to compare against the match location.

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
