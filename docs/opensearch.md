# OpenSearch: match and per-player stats store

**Status: shadow writes only.** The worker keeps OpenSearch in sync, but no API
endpoint reads from it yet. DynamoDB is the source of truth, and everything in
OpenSearch can be rebuilt from it at any time.

## What's stored

| Index (read alias) | One document per | Used for (once reads move over) |
|---|---|---|
| `matches` | match, any status. Each side is nested: team, roster (account holders), outcome, score, conceded, clean sheet | team records, goals for/against, form, head-to-head, match lists |
| `match_stats` | player per **confirmed** match: team, opponent teams, kind, tournament, year, sport, ranked, W/D/L, sport counters under `stats.*` | leaderboards, top performers, any per-player slice |

- The document builders are in `agon_core/src/opensearch/docs.rs`, with their
  unit tests.
- The mappings are in `agon_core/src/opensearch/schema.rs`.
- Players without an Agon account appear in a side's player count but get no
  stats row, because they have no stats identity.

`kind` is derived for now (`league` / `cup` from the tournament format, `squad`
when every side is the same team, `friendly` when at least one side has a team,
otherwise `pickup`), until match setup stores it. `year` is the start time's year
in the tournament's timezone, or UTC.

## How it stays in sync

`agon_worker` runs `handlers/opensearch.rs` on every match `#META` write and
every roster (`PLAYER#`) write. It rebuilds that match's documents from DynamoDB
(`agon_core::opensearch::sync_match`):
1. Index the match document and its player rows in one bulk request.
2. Delete any of that match's player rows that no longer apply.

Likes and comments rewrite `#META` too. When the only change is to those
counters, the handler skips the match without reading anything.

A failure is logged (`opensearch: sync failed`) and **not retried**, because
nothing reads OpenSearch yet and an outage mustn't hold up the stream. Run the
backfill to repair. Revisit this once reads move over.

The shadow write is on when `OPENSEARCH_URL` is set. Optional settings:
`OPENSEARCH_USERNAME`, `OPENSEARCH_PASSWORD` and `OPENSEARCH_INDEX_PREFIX`.

## Running it

**Local:** `docker compose up -d opensearch`, then set
`OPENSEARCH_URL=http://localhost:9200` (already in `.env.example`). On Linux, the
host needs `sudo sysctl -w vm.max_map_count=262144`.

**Cluster (`agon_infra`):** a single-node Deployment, a 10Gi PVC and the
`opensearch` Service.
- It's in-cluster only, with no ingress and the security plugin off.
- The worker gets `OPENSEARCH_URL=http://opensearch:9200`.
- Pulumi also sets `vm.max_map_count` on the node.

Stack config:

| Key | Default | |
|---|---|---|
| `opensearchEnabled` | `true` | `false` removes it and turns the shadow write off |
| `opensearchStorage` | `10Gi` | PVC size |
| `opensearchJavaOpts` | `-Xms1g -Xmx1g` | Heap. The pod requests 1.5 GiB and is limited to 2.5 GiB |

**Check node memory before the first `pulumi up`.** OpenSearch is the largest
thing on the node.

## Backfill

`agon_worker backfill-opensearch` rebuilds every match's documents from
DynamoDB. It's idempotent, so it's safe to re-run at any time.

```bash
agon_worker backfill-opensearch                  # everything, then point the aliases at this version
```

```bash
agon_worker backfill-opensearch --match m_123 --match m_456   # repair specific matches (aliases untouched)
```

```bash
agon_worker backfill-opensearch --resume <cursor>   # continue from a logged cursor
```

Other flags:
- `--concurrency N`: matches synced in parallel. Default 8.
- `--page-size N`: keys examined per scan page. Default 500.
- `--no-swap`: fill the index without moving the aliases.
- `--delete-old`: after the swap, delete the superseded index versions.

It needs only `AGON_TABLE_NAME`, AWS credentials and the `OPENSEARCH_*`
variables, not the worker's SQS, Meilisearch or Temporal settings.
- **In the cluster:** run it as a one-off pod from the worker image, with the
  same `aws-credentials` secret and `OPENSEARCH_URL`.
- **Locally:** `cargo run -p agon_worker -- backfill-opensearch`.

The aliases only move if every match succeeded. Failed matches are logged with
their ids; fix the cause and re-run.

## Changing the schema

OpenSearch can't change a field's type in place, so every mapping change is a
new index version:

1. Edit the mapping in `schema.rs` and **bump that index's `version`**. Update
   the builder in `docs.rs` to match.
2. Deploy the worker. On start it creates `<name>_v<new>` and live changes go
   there. The alias still points at the old version, which is fine while
   nothing reads it.
3. Run `agon_worker backfill-opensearch --delete-old`. It fills the new
   version, points the alias at it, and deletes the old one.

There's one small race to know about. If a match changes at the exact moment
the backfill rewrites it, the backfill's copy can be a moment stale. The next
change to that match, or a re-run, corrects it.

## Integration test

```bash
OPENSEARCH_URL=http://localhost:9200 cargo test -p agon_core --test opensearch_roundtrip -- --nocapture
```

It creates prefixed indices, writes a friendly and a squad game, runs a
leaderboard aggregation and a nested team-record aggregation, and cleans up.
Without `OPENSEARCH_URL` it's skipped.
