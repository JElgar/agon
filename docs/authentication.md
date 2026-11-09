# Authentication overview

Agon uses Supabase Auth for web users. The React app supports email/password and Google OAuth. Supabase issues the session JWT; Agon's API verifies that token and maps its identity to an internal Agon user ID. Paired devices such as Garmin watches use a separate Agon-signed token and do not receive the user's Supabase credentials.

## Components

- **Web UI:** `agon_ui/src/components/auth/LoginForm.tsx` handles password login, signup, and Google OAuth. `agon_ui/src/hooks/useAuth.tsx` loads the Supabase session, tracks auth changes, and signs users out.
- **Supabase client:** `agon_ui/src/lib/supabase.ts` creates the browser client from `VITE_SUPABASE_URL` and `VITE_SUPABASE_ANON_KEY`. These are client configuration values.
- **API client:** `agon_ui/src/lib/api-client.ts` adds the current session's access token as an Authorization bearer header on API requests.
- **Service verifier:** `agon_service/src/main.rs` defines the bearer auth checker; `agon_service/src/auth.rs` verifies JWT signatures and claims.
- **Identity mapping:** `agon_core/src/dao/user.rs` maps an identity-provider `sub` to Agon's internal user ID using an `AUTH#<sub>` guard record.
- **Device pairing:** `agon_core/src/dao/device_pairing.rs`, `agon_service/src/auth.rs`, and the Garmin app implement code confirmation, device identity creation, and device-token issuance.

## Web request flow

1. The user signs in or signs up through Supabase Auth. Signup email confirmation returns to the current URL; Google OAuth also redirects back to it.
2. `useAuth` obtains and tracks the Supabase session.
3. Before an API request, the API client reads the current session and, when an access token exists, sends `Authorization: Bearer <access_token>`.
4. The service's auth checker passes the token to `JwtVerifier`. The verifier requires a `kid` matching a trusted key, allows only ES256 or RS256, and validates the signature, expiry, audience, and required `sub` claim. The default expected audience is `authenticated`; expiry validation has 60 seconds of leeway.
5. Supabase public signing keys are fetched from `SUPABASE_JWKS_URL` on demand and cached. If a token has an unknown `kid`, the verifier refreshes the remote JWKS once to account for key rotation. Invalid tokens return 401.
6. Authenticated handlers resolve the verified `sub` through the `AUTH#<sub>` mapping and use the resulting internal user ID. Signup takes the trusted email from the JWT claim, not from a request body.

## Device request flow

A Garmin watch does not use a password or OAuth. It generates and displays a short pairing code and QR link. A logged-in user confirms the code on Agon's pairing page; the watch polls the service until confirmation, then receives a device access token. The service creates a synthetic device `sub` mapped to the owner's internal user ID.

The device token is an ES256 JWT signed with the dedicated `AGON_DEVICE_JWT_PRIVATE_KEY`. Its public key must be configured in `AGON_DEVICE_JWKS` for the service verifier to trust it. It expires after 365 days, has `device_scope: "live_scoring"`, and is stored by the watch in Garmin application storage. Subsequent watch requests send it as a bearer token. Scoped handlers accept that device scope for the watch's needed endpoints; regular user tokens have no device scope.

## Configuration and local testing

- `SUPABASE_JWKS_URL` configures the Supabase public-key endpoint.
- `AGON_DEVICE_JWT_PRIVATE_KEY` and `AGON_DEVICE_JWKS` configure device-token signing and verification.
- `AGON_STATIC_JWKS` provides a static public-key set for tests/local use; the matching test private key is separate.
- `AGON_JWT_AUDIENCE` overrides the expected audience, which defaults to `authenticated`.
- The UI README documents `VITE_SUPABASE_URL` and `VITE_SUPABASE_ANON_KEY` for the browser client. Local Supabase Auth configuration is described in `local/README.md`.

## Implementation note

The Garmin design document records a residual scope-check gap: some authenticated endpoints do not pass through the scoped-user checks. Device-token scope is enforced on the endpoints explicitly wired for live scoring, but should not be assumed to restrict every authenticated route.
