#!/usr/bin/env node
// Creates the fixed e2e test account against the LOCAL Supabase Auth stack
// (docker-compose.yml's `full` profile — see e2e/README.md's "Running
// fully local" section). The local counterpart to the staging admin-API
// curl in this same README: same GoTrue admin endpoint, just local, and
// authenticated with a service_role token this script mints itself rather
// than a real Supabase project's static SERVICE_ROLE_KEY (this stack has no
// such key — the local Kong gateway skips key-auth entirely, see
// local/supabase/kong.yml's header).
//
// Usage: node agon_ui/e2e/local-seed-user.mjs
// Reads E2E_TEST_EMAIL / E2E_TEST_PASSWORD / E2E_BASE_URL (all optional —
// same defaults `npm run test:e2e` uses locally) from the environment.
import { createPrivateKey, sign } from 'node:crypto'

// Dev-only ES256 keypair, private half. MUST match GOTRUE_JWT_KEYS in
// docker-compose.yml's `supabase-auth` service — if that key is ever
// regenerated, update it here too. Not a secret worth protecting (local
// stack, not reachable from anywhere but this machine), just needs to match.
const GOTRUE_LOCAL_DEV_PRIVATE_JWK = {
  kty: 'EC',
  crv: 'P-256',
  x: 'yXLf-oIMpfA9yLIyoh6lEuF9uFhasekCcNea3KjOIu8',
  y: '7JGtvFl-v5zyS5h2bdbunQA_zL6IL4E_Yjfx-fEZ94I',
  d: 'YbMOJAc9O2VsvKUXN3MRz78R-B_1Z8f0P4i8KPhsl6E',
}
const KID = 'gotrue-local-dev'

const supabaseUrl = process.env.VITE_SUPABASE_URL ?? 'http://localhost:8000'
const email = process.env.E2E_TEST_EMAIL ?? 'e2e@example.com'
const password = process.env.E2E_TEST_PASSWORD ?? 'local-e2e-test-password'

function b64url(buf) {
  return Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

// A short-lived `service_role` token, signed with the same key GoTrue trusts
// for its own tokens (GOTRUE_JWT_KEYS) — GoTrue's admin routes just check the
// caller's `role` claim against GOTRUE_JWT_ADMIN_ROLES, same as a real
// project's static SERVICE_ROLE_KEY would, just minted fresh instead of
// fixed.
function mintServiceRoleToken() {
  const key = createPrivateKey({ key: GOTRUE_LOCAL_DEV_PRIVATE_JWK, format: 'jwk' })
  const now = Math.floor(Date.now() / 1000)
  const header = { alg: 'ES256', typ: 'JWT', kid: KID }
  const payload = { role: 'service_role', aud: 'authenticated', iat: now, exp: now + 60 }
  const signingInput = `${b64url(JSON.stringify(header))}.${b64url(JSON.stringify(payload))}`
  const signature = sign(null, Buffer.from(signingInput), { key, dsaEncoding: 'ieee-p1363' })
  return `${signingInput}.${b64url(signature)}`
}

async function main() {
  const serviceToken = mintServiceRoleToken()
  const res = await fetch(`${supabaseUrl}/auth/v1/admin/users`, {
    method: 'POST',
    headers: {
      apikey: 'local-dev-anon-key',
      Authorization: `Bearer ${serviceToken}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({ email, password, email_confirm: true }),
  })

  if (res.status === 200 || res.status === 201) {
    console.log(`Created local e2e test account: ${email}`)
    return
  }

  const body = await res.text()
  // A second run against an already-seeded stack should be a no-op, not a
  // failure — GoTrue rejects a duplicate email with 422/"already been
  // registered".
  if (res.status === 422 && /already/i.test(body)) {
    console.log(`Local e2e test account already exists: ${email}`)
    return
  }

  console.error(`Failed to create local e2e test account (HTTP ${res.status}): ${body}`)
  console.error(`Is the \`full\` compose profile up? docker compose --profile full up -d`)
  process.exit(1)
}

main()
