# OpenSearch: match, per-player stats and users store

**Status: synced, with three read endpoints.** The worker keeps OpenSearch in
sync. `GET /teams/{id}/summary` and `GET /teams/{id}/leaderboard` read the
match data (see [Reads](#reads)) — the UI doesn't call them yet; switch it
over once the backfill has run. `GET /admin/stats` reads the `users` index
directly for its "total users" figure. DynamoDB is the source of truth
throughout, and every index here can be rebuilt from it at any time.

## What's stored

| Index (read alias) | One document per | Used for |
|---|---|---|
| `matches` | match, any status: kind, tournament with round (and bracket slot for cups), year. Each side is nested: team, roster (account holders), outcome, score, conceded, clean sheet | team records, goals for/against, form, head-to-head, match lists |
| `match_stats` | player per **confirmed** match: team, opponent teams, kind, tournament and round, year, sport, ranked, W/D/L, sport counters under `stats.*` | leaderboards, top performers, any per-player slice |
| `users` | user: id, name, email, creation time, admin flag | the admin panel's total-users count (`GET /admin/stats`, `agon_service/src/admin/mod.rs`) |

- The document builders are in `agon_services_core/src/opensearch/docs.rs`, with their
  unit tests.
- The mappings are in `agon_services_core/src/opensearch/schema.rs`.
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

A failure is logged (`opensearch: match/user sync failed`) and **not
retried**: an outage mustn't hold up the stream, and `GET /admin/stats` and
the team summary/leaderboard endpoints tolerate the same brief staleness
every other async-indexed read in this app already does. Run the backfill to
repair. **Revisit this before the UI switches to the team-stats read
endpoints:** a missed write would then show as a stale record until the next
change to that match.

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

`agon_services_core::opensearch::team_stats` holds the typed queries. Each is a
pure query builder plus a pure response parser, unit-tested without a
cluster, and `tests/opensearch_team_stats.rs` runs them against a real one.
Callers never build query JSON.

| Endpoint (`agon_service/src/team_stats.rs`) | Index | What it reads |
|---|---|---|
| `GET /teams/{id}/summary?sport=&year=` | `matches` | One aggregation for the record per kind (league, cup, friendly, plus `record` = all three, and squad games and goals), the years played, and external matches in the last 12 months (picks `layout`: `standard` from 3, else `squad`). Two small searches for the last 10 results (form, streak, last 10 goals) and the next scheduled match |
| `GET /teams/{id}/leaderboard?sport=&scope=&tournament_id=&year=` | `match_stats` | A `terms` aggregation per player summing W/D/L and every `stats.*` counter. Counter names come from the index mapping. Names and avatars from `batch_get_users`, and `is_member` from the member list (non-members are ringers) |

Only confirmed matches count. Squad games are left out of `record`, form and
last 10, but count on the leaderboard's `all` scope. The API pod gets
`OPENSEARCH_URL` like the worker; without it both endpoints answer 503.

## Changing the schema

OpenSearch can't change a field's type in place, so every mapping change is a
new index version:

1. Edit the mapping in `schema.rs` and **bump that index's `version`**. Update
   the builder in `docs.rs` to match.
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
