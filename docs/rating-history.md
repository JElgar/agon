# Rating history

Goal: show a player how their rating has moved — a chart on a sport's
stats page, and the movement one match caused on that match's detail
page. This doc explains what is already stored, how the profile
endpoints are built today, and why the history is read through its own
endpoint rather than added to the profile response.

**Nothing in `agon_core` needs to change.** Both reads this needs
already exist in the DAO, are already tested, and currently have no
callers. The new code is a handler, a response type and the generated
client.

## Status

Written before any of it was built, and kept because the reasoning is
what a later reader needs — the "what is already built" and "decision"
sections are why the shape is what it is, not a plan.

Since shipped: the history endpoint (Option B below, #332), ratings on
the profile's sports list (#333), the rating card (#334), the chart
(#337, rescaled in #351) and the ranked markers on match detail and the
create flow (#335, #336).

Still open, all of them listed under "Follow-ups and risks" at the end:
the `RepairRatings` replay, `ranked` on `SearchMatch` and on
`POST /match-series`, and the match-detail delta question in
"Visibility".

## What is already built

The rating pipeline has been writing a per-match history item since it
shipped. Nothing is recomputed on the fly and nothing is discarded.

`USER#<uid>` / `RATING#<ladder>#<played_at>#<matchId>` →
`RatingHistoryRecord` (`agon_core/src/dao/records.rs:1463`) holds the
ladder, the match id, the match's `starts_at`, the wall clock it was
applied at, and a `RatingMovementRecord`: `mu_before`, `sigma_before`,
`mu_after`, `sigma_after` and `display_delta`.

Three properties of that record matter for the chart:

- **It carries σ, not just μ.** Every point knows its own confidence
  band, so the chart can show the `±` narrowing as a player settles,
  which is the actual story of a new rating — not just a line.
- **The key sorts on played order**, not confirmation order. A Monday
  game confirmed on Thursday sorts before Wednesday's, so the
  collection is already a correct left-to-right timeline with no
  client-side sorting.
- **`display_delta` is stored even though μ implies it.** Deliberate:
  the displayed rating is computed through `rating::scale` on every
  read so the scale stays retunable by deploy, but a log entry records
  what the player was actually told on the day. A recomputed delta
  would quietly rewrite history if the scale were ever retuned.

The write happens in the same transaction as the rating itself
(`rating_contribution_items` in `agon_core/src/dao/rating.rs`, item 3
of 4), derived from the contribution by
`RatingContributionRecord::history_entry` so the two can't disagree.
Item 4 of that transaction relocates the history entry if a match is
rescheduled or its sport is edited, so the timeline doesn't rot and
can't pick up duplicates.

The read paths exist too:

- `Dao::list_rating_history(user_id, ladder, from, cursor, limit)`
  (`agon_core/src/dao/rating.rs:372`) — ascending, paginated, bounded
  to one ladder by a `BETWEEN` on the sort key. **Zero callers outside
  its own module.**
- `Dao::get_rating_contribution(match_id, user_id)`
  (`agon_core/src/dao/rating.rs:304`) — a single `GetItem` on an exact
  key, for one match's effect on one player.

Both were written for the planned `RepairRatings` replay and for this
chart. The replay isn't built; the chart is what's being built now.

## How the profile works today

This is the part worth understanding before deciding where history
goes, because the profile response is the hottest read in the app and
it is cheap for a specific reason.

### One item, one read

Everything in `UserProfile` comes off a single DynamoDB item,
`USER#<uid>` / `#PROFILE`: the display name, the image URL, the
follower and following counts, the per-sport `UserStats` (generated
from the sport list by `define_score_and_match_type!`), and the
`ratings` map.

So the endpoints are point reads, not queries:

- `GET /users/me` (`agon_service/src/main.rs:2551`) — two point reads:
  the `AUTH#<sub>` guard to resolve the token's `sub` to the internal
  id, then the profile item.
- `GET /users/:user_id` (`agon_service/src/main.rs:2720`) — three: the
  profile item, the caller's own id, and the follow edge for
  `is_followed_by_me`.

### The mapper is pure

`user_profile_from_record(&UserRecord, is_followed_by_me) ->
UserProfile` (`agon_service/src/mapping.rs:405`) is a synchronous,
side-effect-free function. It takes a record and a bool and returns the
DTO. It has no DAO handle and is not `async`.

That purity is load-bearing, because `UserProfile` is not a
single-endpoint shape. It is built in **eight** places and returned in
bulk:

- `Vec<UserProfile>` from user search (`main.rs:2995`)
- `Vec<UserProfile>` from the followers and following lists
  (`main.rs:8548`)
- `known_participants: Vec<UserProfile>` embedded **inside match
  responses** (`main.rs:1312`)
- plus `/users/me`, `/users/:user_id`, signup, and the invitation and
  profile lookups (`main.rs:7769`, `main.rs:8741`)

### Where the placements rule is enforced

`published_ratings_from_record` (`agon_service/src/mapping.rs:421`) is
the single chokepoint. A stored rating exists for every ladder a user
has played a ranked match on; only those past placements may leave the
server, and that filter is applied in one function. Its own doc
comment says why it lives there rather than in each caller: *"rather
than left to each caller to remember, which is how an unplaced number
eventually reaches a response."*

The own-view counterpart, `rating_placements_from_record`
(`mapping.rs:469`), is likewise pure, and a ladder appears in exactly
one of `UserProfile::ratings` and `User::rating_placements`, never
both.

### How the client caches it

`agon_ui` has two query keys over these endpoints:

- `['users-me']` via `currentUserQuery` — read by `useCurrentUserId`,
  `useCurrentUserProfile`, the feed banner and the create flow, all
  sharing one fetch through `select`.
- `['profile', id]` via `useProfileView` — the profile page and each
  sport's stats page.

Between them, the profile response is on the hot path of nearly every
screen in the app.

## The decision

### Option A — inline history on the profile response

Nest the points under each entry of `UserProfile::ratings`.

This is the option that looks simpler and isn't. The profile item
carries no history, so the data has to come from a query — and
`UserProfile` is returned in lists:

- A 20-result user search would need a rating-history query **per user
  per ladder**. For five-sport users that is 100 queries behind one
  search request. The same applies to the followers and following
  lists.
- Every match detail response embeds `known_participants`, so each
  would carry every participant's full rating history on every read.
- With today's DAO it is one query per ladder, because the sort key is
  ladder-scoped. Avoiding that fan-out would mean a new DAO function
  that queries the whole `RATING#` collection at once — reading a
  user's entire history across every sport on every profile load. That
  is worse, and it is exactly the DAO change we don't want to make.
- `user_profile_from_record` would have to become `async` and take a
  DAO handle, or all eight call sites would have to fan out and pass
  history in. Either way the pure mapper is gone.
- The placements rule would move from one chokepoint into a nested
  field of a DTO built in eight places. That is precisely the failure
  mode `published_ratings_from_record`'s doc comment warns about, on
  the one rule whose whole purpose is that it must never leak.

### Option B — a separate endpoint per (user, ladder)

`GET /users/:user_id/ratings/:ladder/history`.

- The profile endpoints stay point reads. Search, follower lists and
  match responses are untouched.
- The mapper stays pure. Nothing about `UserProfile` changes.
- One handler owns the visibility rule, for one user and one ladder, at
  the only place history is ever served.
- It is lazy. Only a sport's stats page draws a chart, so only that
  page pays a round trip, on its own cache key
  (`['rating-history', userId, ladder]`).
- The payload is bounded by one ladder, and the client already knows
  how many points to expect before asking: `LadderRating.matches_rated`
  is on the profile it has already loaded.

Cost: one handler, one response type, a schema regen and one generated
client model. Plus a public surface to keep — and since the DAO is
already paginated, exposing the cursor now is nearly free and avoids a
breaking change later.

### Option C — defer

Reasonable only if the chart isn't being built. The data is written
either way, so waiting costs nothing but leaves the boards unbuilt.

### Chosen: Option B

Two reads, both over DAO functions that already exist:

| Screen | Endpoint | Cost |
|---|---|---|
| Sport stats chart | `GET /users/:user_id/ratings/:ladder/history` | one `Query`, ladder-scoped, paginated |
| Match detail movement | `display_delta` on the `Match` response | one `GetItem` |

The match-detail movement deliberately does **not** come from the
history endpoint. Fetching a whole ladder's timeline to render one
match's `+18` is the wrong shape. It is one point read on an exact key
in the match's own partition, next to the two reads `get_match`
already does.

## Visibility

The rule is the same everywhere, and it is the placements rule
unchanged: **nothing numeric about a rating leaves the server until
that rating is published.**

- History is written from the first ranked match (it already is).
- The endpoint returns nothing for a ladder still in placements — to
  anyone, **including the owner**. That matches
  `published_ratings_from_record`: a rating mid-placements is withheld
  from the player it belongs to too.
- Once published, the chart shows the **whole** path, placement
  matches included. The early points have a wide `±` narrowing as the
  player settles, which is the honest version of the story and the
  most interesting part of the chart.
- For someone else's profile, the same single condition applies: the
  ladder is published, or there is no history to read.

### The open question: the match-detail delta during placements

A `display_delta` is not a position, so showing `+18` during
placements looks harmless. It isn't quite: deltas accumulate from a
known origin (1500), so a player who can see every delta can sum them
back to the number being withheld.

Recommendation: **withhold the delta during placements too**, and show
progress instead — "Counts towards your placements · 3 of 5". One rule,
no arithmetic back door, and the match page still gives a player
something that moved. Revisit only if placements progress turns out to
be too weak a signal in practice.

## What this deliberately does not do

- **No DAO changes.** No new functions, no changed signatures, no key
  changes. `list_rating_history` and `get_rating_contribution` are used
  as they stand.
- **No descending mode.** `list_rating_history` is ascending only, on
  purpose: a replay walks forwards and a chart plots left to right. A
  reverse direction would be a second sort order to keep correct for a
  reversal the client can do on a page it already holds.
- **No rating-contribution endpoint for other players.** Only the
  viewer's own movement is exposed on a match, so a participant still
  in placements can't have theirs read off someone else's screen.
- **No `RepairRatings`.** Out-of-order results are detected and handed
  to a replay that isn't built. A chart makes that visible where a
  single current number hides it — see below.

## Follow-ups and risks

- **Out-of-order history is visible on a chart.** A result confirmed
  late is still applied, and until `RepairRatings` exists the stored
  path can disagree with what a strict replay would produce. One
  number hides this; a line does not. Worth knowing before the chart
  ships, and an argument for building the replay sooner.
- **`SearchMatch` has no `ranked` field**, so a ranked chip can't be
  shown in the feed or any match list — only on the detail page. One
  field on the server unblocks it everywhere.
- **`POST /match-series` has no `ranked` field**, so a series can't be
  created ranked.
- **`CLAUDE.md` references `docs/dynamodb-design.md`**, which doesn't
  exist in the repo. Unrelated to this change, but noted while looking
  for it.
