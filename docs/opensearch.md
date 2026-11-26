# OpenSearch: match, per-player stats and users store

**Status: synced, with four read endpoints.** The worker keeps OpenSearch in
sync. `GET /teams/{id}/summary` and `GET /teams/{id}/leaderboard` read the
match data (see [Reads](#reads)) — the UI doesn't call them yet; switch it
over once the backfill has run. The whole admin panel's user resource —
`GET /admin/stats` (total-users count) and `GET /admin/users` (search/browse)
— reads the `users` index; Meilisearch's `users` index is no longer in the
admin panel's path (the user-facing `GET /users/search` is separate and still
on Meilisearch). DynamoDB is the source of truth throughout, and every index
here can be rebuilt from it at any time.

## What's stored

| Index (read alias) | One document per | Used for |
|---|---|---|
| `matches` | match, any status: kind, tournament with round (and bracket slot for cups), year. Each side is nested: team, roster (account holders), outcome, its scoreline under `sides.result.<sport>` (football goals for/against, cricket runs and wickets for/against), and the sport's own counters under `sides.stats.<sport>` (its players' `match_stats.stats` summed; football's `clean_sheets` is the side's, once) | team records, scorelines for/against, recent results, head-to-head, match lists, per-side sport breakdowns (cricket wickets, football assists, ...) |
| `match_stats` | player per **confirmed** match: team, opponent teams, kind, tournament, round and series, year, sport, ranked, W/D/L, the sport's own counters under `stats.<sport>` | leaderboards, top performers, any per-player slice |
| `users` | user: id, name (text), email (text), creation time, admin flag | the admin panel's total-users count and its user search/browse (`GET /admin/stats`, `GET /admin/users`, `agon_service/src/admin/mod.rs`) |

- The document builders are in `agon_services_core/src/opensearch/docs.rs`, with their
  unit tests.
- The mappings are in `agon_services_core/src/opensearch/schema.rs`.
- Each sport's counters are its own type in its sport file
  (`agon_services_core::sports::<sport>`, `SportRecord::MatchStatsRecord`),
  built from the same per-match contribution as lifetime stats. The
  mapping lists each sport's fields explicitly
  (`SportRecord::MATCH_STATS_FIELDS`), and the reads sum only the asked-for
  sport's fields into that type. A side's scoreline (`result`) works the
  same way (`SportRecord::SideResultRecord`, read from the confirmed score).
  A generic sport (tennis, badminton, ...) has neither, only W/D/L.
- Players without an Agon account appear in a side's player count but get no
  stats row, because they have no stats identity.

`kind` is derived for now (`league` / `cup` from the tournament format, `squad`
when every side is the same team, `friendly` when at least one side has a team,
otherwise `pickup`), until match setup stores it. `year` is the start time's year
in the tournament's timezone, or UTC.

## Client and typing

`agon_services_core::opensearch::OpenSearchClient` wraps the official
[`opensearch`](https://crates.io/crates/opensearch) crate. That crate is
untyped for bodies (JSON in, JSON out), so the typing lives in our module, and
callers never build query JSON themselves:
- **Documents** are typed structs in `docs.rs`.
- **Index shapes** are defined once, in `schema.rs`.
- **Read queries**, when they arrive, will be typed functions returning typed
  rows in the same module.

## How it stays in sync

`agon_worker` runs `handlers/opensearch.rs` on every match `#META` write and
every roster (`PLAYER#`) write. It rebuilds that match's documents from DynamoDB
(`agon_services_core::opensearch::sync_match`):
1. Index the match document and its player rows in one bulk request.
2. Delete any of that match's player rows that no longer apply.

Likes and comments rewrite `#META` too. When the only change is to those
counters, the handler skips the match without reading anything.

The same handler also runs on every user `#PROFILE` write, rebuilding that
user's `users` document (`agon_services_core::opensearch::sync_user`) — just
an index, no player rows or stale-row cleanup to do.

A failed sync doesn't fail the stream message (that would replay every other
handler and, during an outage, send events to the DLQ). It's handed to a
Temporal **`RepairOpenSearch`** workflow instead (`os-repair-match-<id>` /
`os-repair-user-<id>`):
- Its activity rebuilds the documents from DynamoDB's current state, so a late
  retry never writes stale data.
- It retries with backoff (5s, doubling, capped at 10 minutes) for up to
  three days. Past that, run the backfill with `--match <id>`.
- A second failure for the same target while a repair is still retrying
  attaches to that run.
- Only if Temporal itself can't be reached does the message fail and
  redeliver.

Watch for `opensearch: sync failed; scheduling a repair` and
`opensearch repair succeeded` in the worker logs. Local runs without
Temporal just log the failure (run the backfill to repair).

The shadow write is on when `OPENSEARCH_URL` is set. Optional settings:
`OPENSEARCH_USERNAME`, `OPENSEARCH_PASSWORD` and `OPENSEARCH_INDEX_PREFIX`.
`agon_service` needs this too now (not just `agon_worker`), since `GET
/admin/stats` reads the `users` index directly — it reports 503 while
`OPENSEARCH_URL` is unset, same convention as the other optional integrations
(device pairing, YouTube).

## Running it

**Local:** `docker compose up -d opensearch`, then set
`OPENSEARCH_URL=http://localhost:9200` (already in `.env.example`). On Linux, the
host needs `vm.max_map_count` of at least 262144. Many distros already ship
more, so check `sysctl vm.max_map_count` first and only then run
`sudo sysctl -w vm.max_map_count=262144`. The container uses about 900 MiB.

**Fully local, with DynamoDB Local:**

```bash
docker compose up -d dynamodb-local dynamodb-local-init opensearch
make test-opensearch-local       # both integration tests below
make backfill-opensearch-local   # backfill from DynamoDB Local's `agon` table
```

Both targets set `AWS_ENDPOINT_URL=http://localhost:8002` with dummy
credentials, so they can't reach a real table whatever `.env` says. DynamoDB
Local is in-memory and starts empty, so the backfill only has something to
index once you've created matches locally, for example by running
`agon_service` and `agon_worker` against the local stack (`local/README.md`).

**Cluster (`agon_infra`):** a single-node Deployment, a 10Gi PVC and the
`opensearch` Service.
- It's in-cluster only, with no ingress and the security plugin off.
- The worker gets `OPENSEARCH_URL=http://opensearch:9200`.
- Pulumi also sets `vm.max_map_count` on the node.

It's always deployed. Optional stack config:

| Key | Default | |
|---|---|---|
| `opensearchStorage` | `10Gi` | PVC size |
| `opensearchJavaOpts` | `-Xms1g -Xmx1g` | Heap. The pod requests 1.5 GiB and is limited to 2.5 GiB |

**Check node memory before the first `pulumi up`.** OpenSearch is the largest
thing on the node.

## Backfill

`agon_worker backfill-opensearch` rebuilds every match's and user's documents
from DynamoDB, in the same run. It's idempotent, so it's safe to re-run at any
time.

```bash
agon_worker backfill-opensearch                  # everything, then point the aliases at this version
```

```bash
agon_worker backfill-opensearch --match m_123 --match m_456   # repair specific matches (aliases untouched)
```

```bash
agon_worker backfill-opensearch --resume <cursor> --resume-users <cursor>   # continue from logged cursors
```

Other flags:
- `--concurrency N`: matches and users synced in parallel (each has its own pool). Default 8.
- `--page-size N`: ids per page read from the entity index. Default 500.
- `--no-swap`: fill the indices without moving the aliases.
- `--delete-old`: after the swap, delete the superseded index versions.

`--match` targets specific matches only — there's no analogous per-user flag
yet, since nothing has needed to repair one user's document in isolation.

It needs only `AGON_TABLE_NAME`, AWS credentials and the `OPENSEARCH_*`
variables, not the worker's SQS, Meilisearch or Temporal settings.
- **In the cluster:** run it as a one-off pod from the worker image, with the
  same `aws-credentials` secret and `OPENSEARCH_URL`.
- **Locally:** `make backfill-opensearch-local` reads DynamoDB Local. Plain
  `cargo run -p agon_worker -- backfill-opensearch` reads whatever DynamoDB the
  `AWS_*` settings point at.

It lists matches and users from the entity index (GSI4), not a table scan. On
a table that predates the index, run the entity-index migration first
(`claude/entity-index-migration`), or items without index keys are skipped.
`--match` reads matches directly and doesn't need the index.

It logs `reading dynamodb` with the table and endpoint before it does
anything, so check that line when you're not sure which table you're on.

The aliases only move if every match *and* every user succeeded. Failures are
logged with their ids; fix the cause and re-run.

## Reads

`agon_services_core::opensearch::team_stats`, `::user_stats` and `::users`
hold the typed queries. Each is a pure query builder plus a pure response
parser, unit-tested without a cluster; `team_stats`'s also run against a real
cluster in `tests/opensearch_team_stats.rs`. Callers never build query JSON.

| Endpoint | Index | What it reads |
|---|---|---|
| `GET /teams/{id}/summary?sport=&year=` (`agon_service/src/team_stats.rs`) | `matches` | One aggregation for the record per kind (league, cup, friendly, plus `record` = all three, and squad games), each kind's `sides.result.<sport>` scoreline summed into `result` and `sides.stats.<sport>` counters into `stats`, the years played, and external matches in the last 12 months (picks `layout`: `standard` from 3, else `squad`). Two small searches for the last 10 results (`recent_results` and streak; each result's score is read from DynamoDB in its sport's own shape, as on the feed) and the next scheduled match |
| `GET /teams/{id}/leaderboard?sport=&scope=&tournament_id=&year=` (`agon_service/src/team_stats.rs`) | `match_stats` | A `terms` aggregation per player summing W/D/L and the sport's `stats.<sport>` counters. Names and avatars from `batch_get_users`, and `is_member` from the member list (non-members are ringers) |
| `GET /users/{id}/summary?sport=&year=` (`agon_service/src/user_stats.rs`) | `match_stats` | A player's own record per kind, summed straight off `match_stats` (one row per confirmed match for that player already) — no nested aggregation needed, unlike the team version. `matches` is used only for the next scheduled match |
| `GET /admin/users?q=&cursor=&limit=` (`agon_service/src/admin/mod.rs`) | `users` | A `multi_match` over `name`/`email` (prefix on the last term, `bool_prefix`), or `match_all` for an empty `q`, sorted newest-first; offset pagination like the old Meilisearch-backed version |
| `GET /admin/stats` (`agon_service/src/admin/mod.rs`) | `users` | An exact `_count` for the total-users figure |

Only confirmed matches count. Squad games are left out of a team's `record`,
form and last 10, but count on the leaderboard's `all` scope. For a player's
own summary, squad games keep a real W/D/L (they were only ever on one of the
two sides), but are still kept out of `record`/form/last10, same convention
as the team version. There's no per-player leaderboard — a leaderboard is
inherently team-scoped — and no personal goals-for/against or clean sheets,
since those are side-level, not individually attributable. The API pod gets
`OPENSEARCH_URL` like the worker; without it every one of these answers 503.

## Changing the schema

OpenSearch can't change a field's type in place, so every mapping change is a
new index version:

1. Edit the mapping in `schema.rs` and **bump that index's `version`**. Update
   the builder in `docs.rs` to match. Bump it even for a field that's "new,
   nothing's deployed yet" — that's only true for a fresh environment. On
   staging, `tournament_round`/`tournament_slot` were added to `matches`'
   mapping without a version bump; `matches_v1` already existed there from
   before, `ensure_index` never updates an existing index's mapping, and the
   next tournament match indexed hit `strict_dynamic_mapping_exception`.
   `matches` has since moved on to v3 (the per-sport `sides.result`/`sides.stats`
   rework) and `match_stats` to v3 (adding `series_id`, the per-player
   analogue of `tournament_id`, for a future series-scoped slice) — each a
   version bump of its own for the same reason.
2. Deploy the worker. On start it creates `<name>_v<new>` and live changes go
   there. The alias, and so the read endpoints, still point at the old
   version, which stops getting live changes. Reads are stale until step 3
   finishes, so run it straight after the deploy.
3. Run `agon_worker backfill-opensearch --delete-old`. It fills the new
   version, points the alias at it, and deletes the old one.

There's one small race to know about. If a match changes at the exact moment
the backfill rewrites it, the backfill's copy can be a moment stale. The next
change to that match, or a re-run, corrects it.

## Integration tests

```bash
make test-opensearch-local
```

- `opensearch_roundtrip` needs only OpenSearch. It creates prefixed indices,
  writes a friendly and a squad game, runs a leaderboard aggregation and a
  nested team-record aggregation, and cleans up.
- `opensearch_dynamodb_sync` also needs DynamoDB Local. It writes a confirmed
  match through the DAO and runs `sync_match` three times: once to index it,
  again after a player leaves (their row must go), and again after the match
  is deleted (all of its documents must go).

Each is skipped without `OPENSEARCH_URL`. The sync test is also skipped
without `AWS_ENDPOINT_URL`, so it never writes to a real table.
