# `local/`

Config for the backing services `docker-compose.yml`'s `full`/`dynamodb`
profiles stand up — everything beyond Meilisearch: local Supabase Auth,
local Temporal, and DynamoDB Local. See `agon_ui/e2e/README.md`'s "Running
fully local" section for how the Supabase/Temporal pieces fit together and
how to actually run something against that stack.

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

**Known gap:** DynamoDB Streams aren't enabled on the local table, so
anything that depends on `agon_worker`'s async pipeline (search indexing,
feed fan-out) won't happen against this stack — `agon_worker` reads off an
SQS queue fed by a real EventBridge Pipe in production/staging, and there's
no local equivalent of that pipe. In practice this means ~20 of
`agon_tests`' 82 tests (anything asserting something becomes searchable or
lands in a feed) will time out and fail even with DynamoDB Local up — that's
expected, not a sign anything here is broken. Standing up a local
stream-to-queue bridge would close this gap but is a separate, bigger piece
of work than DynamoDB Local itself.

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
