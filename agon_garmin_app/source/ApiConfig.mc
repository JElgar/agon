import Toybox.Lang;

//! The one backend base URL every API client in this app (pairing, match
//! lookup, live scoring) builds its requests against. File-scope, not a
//! class `const` — a class's own `const` isn't reachable via
//! `ClassName.CONST` at all (see DeviceAuth.mc's doc comment for the real
//! compiler errors that established this), and a single shared constant
//! also means there's exactly one place to change if this ever needs to
//! point elsewhere.
//!
//! `agon_service` mounts its own routes at `/` — every deployed
//! environment (staging included) fronts it with an ingress that
//! publishes that under `/api` and strips the prefix before it reaches
//! the service (see `agon-api-ingress` in `agon_infra/index.ts`, and the
//! same `/api` convention `agon_ui`'s dev proxy and `make test-staging`
//! both use). For local dev instead, point this at
//! `http://localhost:7000` (no `/api` — a local `agon_service` has no
//! ingress in front of it) and run `make run`; the Connect IQ simulator's
//! network requests are proxied through the desktop it runs on, so
//! `localhost` reaches it directly.
const API_BASE_URL = "https://agon.staging.get-agon.com/api";
