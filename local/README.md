# `local/`

Config for the backing services `docker-compose.yml`'s `full`/`dynamodb`
profiles stand up — everything beyond Meilisearch: local Supabase Auth,
local Temporal, and DynamoDB Local + SQS (ElasticMQ). See `agon_ui/e2e/
README.md`'s "Running fully local" section for how the Supabase/Temporal
pieces fit together and how to actually run something against that stack.

## `local/supabase/`

A trimmed self-hosted Supabase Auth (Postgres + GoTrue + Kong), cut down from
[Supabase's own reference self-hosting setup](https://github.com/supabase/supabase/tree/master/docker)
to just what `agon_ui`/`agon_service` actually touch — the app's only
browser-side Supabase surface is `supabase.auth.*` (see
`agon_ui/src/lib/supabase.ts`), so Storage/Realtime/PostgREST/Studio/
Analytics are all left out.

- `roles.sql`, `jwt.sql` — Postgres init scripts (from Supabase's reference
  setup, trimmed to the roles this image/stack actually has — see
  `roles.sql`'s own header for why).
- `auth-ownership.sql` — reassigns a few pre-baked `auth.*` helper functions
  to `supabase_auth_admin`, which GoTrue's own migrations need to own. See
  the file's header for why this is needed at all.
- `kong.yml` — one passthrough route (`/auth/v1/*` → GoTrue), not a copy of
  Supabase's production Kong config. See the file's own header for why.

### The signing keypair

`docker-compose.yml`'s `supabase-auth` service signs session JWTs with a
dedicated ES256 (P-256) keypair, generated once for this repo and committed
directly in `GOTRUE_JWT_KEYS` (dev-only, not a secret worth rotating — this
stack is never reachable from anywhere but your own machine). `agon_service`
trusts it the same way it'd trust a real Supabase project: via
`SUPABASE_JWKS_URL` pointed at this stack's `/auth/v1/.well-known/jwks.json`
(GoTrue derives and serves the public half itself — nothing else needs to
know the key material). `agon_ui/e2e/local-seed-user.mjs` holds its own copy
of the private half, to mint a short-lived `service_role` token for GoTrue's
admin API (this stack has no Kong key-auth / static `SERVICE_ROLE_KEY` to use
instead — see `kong.yml`'s header).

If this key ever needs regenerating (compromise isn't really a concern
locally, but e.g. switching to RS256, or just wanting a fresh one), generate
a fresh P-256 keypair as a JWK and update both places — GoTrue requires
`key_ops: ["sign"]` on the private key or it refuses to start
("no signing key detected"):

```js
const crypto = require('node:crypto')
const { privateKey } = crypto.generateKeyPairSync('ec', { namedCurve: 'P-256' })
const jwk = privateKey.export({ format: 'jwk' })
console.log(JSON.stringify({
  kty: jwk.kty, crv: jwk.crv, alg: 'ES256', use: 'sig',
  key_ops: ['sign'], kid: 'gotrue-local-dev',
  x: jwk.x, y: jwk.y, d: jwk.d,
}))
```

Paste the result into `docker-compose.yml`'s `GOTRUE_JWT_KEYS` **and**
`agon_ui/e2e/local-seed-user.mjs`'s `GOTRUE_LOCAL_DEV_PRIVATE_JWK` — they
must match.

## `local/` (Temporal)

No config files needed — `temporalio/auto-setup` runs its own schema
migrations and registers the `default` namespace on first boot.
`docker-compose.yml`'s `temporal`/`temporal-db`/`temporal-ui` services are
self-contained.

## `local/dynamodb/`

DynamoDB is real/cloud by default (see the root `CLAUDE.md`) — most work
doesn't need a local table, and one less thing to keep in sync with what
staging actually runs on. This is for `make test` (`agon_tests`) without AWS
credentials, or working on `agon_worker`'s DynamoDB-facing code without
touching a real table.

`docker compose --profile dynamodb up -d` (also included in `--profile
full`) starts `amazon/dynamodb-local` (`-inMemory` — state resets on every
restart) plus a one-shot `dynamodb-local-init` container that creates the
`agon` table via `create-table.sh`. That script's schema (`PK`/`SK`, GSI1-3,
TTL on `ttl`) is hand-mirrored from the real table's Pulumi definition in
`agon_infra/index.ts` (`dynamoTable`) — there's no way to detect drift
between them automatically, so if the real schema ever changes, mirror the
change here too.

Point `agon_service`/`agon_tests`/`agon_worker` at it by uncommenting
`AWS_ENDPOINT_URL` (and the local `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`
right above it) in `.env` — see `.env.example`. No code changes needed on
the Rust side: `aws_config::load_from_env()` already honors
`AWS_ENDPOINT_URL` out of the box.

`elasticmq` (the `elasticmq`/`elasticmq-init` services — see
`create-queues.sh`) is the local stand-in for SQS, giving `agon_worker`
somewhere to long-poll (`AGON_EVENTS_QUEUE_URL`/`AGON_ASSET_EVENTS_QUEUE_URL`
— see `.env.example`'s `agon_worker` section). Streaming is enabled on the
table (`create-table.sh`), but by itself that only means the stream *exists*
— something still has to read it and forward records to `elasticmq`, the way
a real EventBridge Pipe does in production (`agon_infra/index.ts`). That's
`local/dynamodb-stream-bridge` — see its own section below, and
`agon_ui/e2e/README.md`'s "Running fully local" section for `make
run-stream-bridge`'s place in the overall local run sequence. Without it
running, `agon_worker` starts and idles correctly, just never receives
anything — search indexing and feed fan-out won't happen, and the
`agon_tests` cases asserting on those will time out.

### S3 (asset uploads)

`minio`/`minio-init` (see `create-bucket.sh`) is the local stand-in for S3,
serving presigned PUTs the way `agon_service` issues them
(`agon_service/src/assets.rs`). This needed one real code change, not just
infra: a real S3 presigned URL is virtual-hosted-style
(`<bucket>.<endpoint>/<key>`), which MinIO has no wildcard DNS for — a PUT
against one just DNS-fails. `AGON_ASSETS_S3_FORCE_PATH_STYLE` (see
`.env.example`) switches `agon_service`'s S3 client to path-style
(`<endpoint>/<bucket>/<key>`) instead, gated behind that env var so real
deployments (which always use virtual-hosted-style) are untouched. Verified
this concretely — a real presigned PUT against MinIO, replayed exactly the
way `agon_tests` does it — before making the change.

MinIO requires *validated* credentials (unlike `dynamodb-local`/`elasticmq`
above, which accept anything) — `MINIO_ROOT_PASSWORD` must be 8+ characters,
which is why every `AWS_SECRET_ACCESS_KEY` in this profile is `localsecret`
rather than the shorter `local` used before MinIO was added. One shared
credential pair across the whole `dynamodb` profile is simpler to document
than a special case just for this service.

**Known gap:** the presigned PUT itself works, but `agon_tests`'
`upload_*_end_to_end`/`attach_*` cases still fail — they wait for
`agon_worker` to mark the asset `uploaded`, which only happens once
`agon_worker`'s asset consumer receives an S3-object-created notification
over `agon-asset-events` (see `agon_worker/src/asset_consumer.rs`). That's
S3 → EventBridge → SQS in production, and nothing here stands in for that
leg — MinIO *can* be configured to POST bucket-event webhooks
(`MINIO_NOTIFY_WEBHOOK_*`), but its payload shape (`Records[].s3.object.key`)
doesn't match what `asset_consumer.rs` expects (EventBridge's
`detail.object.key` wrapper), so closing this needs a second small
translator tool, the same shape as `dynamodb-stream-bridge` below — not yet
built.

## `local/dynamodb-stream-bridge/`

A separate, standalone Cargo project (its own `[workspace]`, excluded from
the root one the same way `agon_tests` is — see the root `Cargo.toml`) that
tails DynamoDB Local's stream for the `agon` table and forwards each record
to `elasticmq`, in the shape `agon_worker/src/event.rs` expects. The local
stand-in for the EventBridge Pipe in `agon_infra/index.ts` — see its own
`src/main.rs` doc comment for the full design, and `agon_ui/e2e/README.md`'s
"Running fully local" section / `make run-stream-bridge` for how to actually
run it (a separate long-lived process, same as `agon_service`/`agon_worker`
— not a compose service, deliberately: it's Rust dev tooling, and running it
the same way you run the real binaries keeps one consistent local-dev
pattern instead of two).

Worth knowing if you ever touch this tool: it does **not** use the generated
`aws-sdk-dynamodbstreams` client for the actual `GetRecords` call (stream
discovery — `ListStreams`/`DescribeStream`/`GetShardIterator` — still does).
DynamoDB Local has a real interop bug where it renders an empty Map
attribute in a stream record as bare `{}` instead of the tagged `{"M": {}}`
real DynamoDB uses, which the generated client's strict typed deserializer
rejects outright — failing the *entire* batch, not just that one attribute.
Since `agon_core`'s records commonly carry an empty map early on (e.g. a
fresh user's `stats: {}`), this hit nearly every record, not some rare edge
case. The fix: a raw HTTP call for `GetRecords` specifically, parsed as
untyped JSON and patched (`patch_image`/`patch_empty_maps`) before
forwarding. DynamoDB Local doesn't validate SigV4 signature *content*
(confirmed empirically), so the "signed" request is real-shaped but not
cryptographically real — fine against DynamoDB Local, would need actual
signing to work against real AWS (out of scope; this tool only ever exists
to bridge a local table).

### The test-signing key

`agon_tests` needs `AGON_TEST_JWT_PRIVATE_KEY` to sign tokens and
`AGON_STATIC_JWKS` (in `.env`) for `agon_service` to trust them — see
`agon_service/src/auth.rs`. The private key lives in `agon-test-key.pem`
here, **not** in `.env`/`.env.example`, for a real reason: `.env` is read by
both Make (`include .env`) and Docker Compose (auto-loaded for variable
substitution), and their parsers disagree — Make has no way to hold a real
multi-line value except a `define`/`endef` block, which Compose's parser
rejects outright ("key cannot contain a space"), breaking every `docker
compose` command stack-wide. A `"...\n...\n..."`-escaped single line avoids
that specific breakage but doesn't actually work either: neither Make's
`include` nor the Rust code that reads this var unescape `\n`, so it fails
to parse as PEM (this was `.env.example`'s previous, silently-broken,
documented format). The Makefile picks this file up as
`AGON_TEST_JWT_PRIVATE_KEY`'s default (`?=`, so `test-staging`'s own
recipe-level override still wins) — nothing needs to be set in `.env` for
this at all, only `AGON_STATIC_JWKS` (a single JSON line — safe for both
parsers) to opt in.
