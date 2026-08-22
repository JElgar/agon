# `local/`

Config for the backing services `docker-compose.yml`'s `full` profile stands
up — everything beyond Meilisearch: local Supabase Auth and local Temporal.
See `agon_ui/e2e/README.md`'s "Running fully local" section for how the
pieces fit together and how to actually run something against this stack.

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
