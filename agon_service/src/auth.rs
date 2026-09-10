//! JWT verification for the API.
//!
//! Tokens are signed **asymmetrically** (ES256/RS256) and verified against a set
//! of trusted public keys — never a shared secret. Two trust sources are
//! combined:
//!
//! 1. **Supabase JWKS** (production): real user tokens are signed by Supabase's
//!    rotating asymmetric keys, published at
//!    `https://<project>.supabase.co/auth/v1/.well-known/jwks.json`. Set via
//!    `SUPABASE_JWKS_URL`. Fetched lazily and cached; on an unknown `kid` we
//!    refetch once (key rotation) before rejecting.
//! 2. **Static JWKS** (tests / local): a JSON JWK Set in `AGON_STATIC_JWKS`
//!    holding the public half of the dedicated test keypair. The integration
//!    tests and the `generate-token` CLI sign with the matching private key
//!    (`AGON_TEST_JWT_PRIVATE_KEY`), so this is the asymmetric equivalent of the
//!    old shared `JWT_SECRET` — an isolated, test-only trust anchor.
//!
//! A token verifies if *either* source has a key whose `kid` matches its header.

use std::sync::{Arc, RwLock};

use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Claims we read off a verified token. `sub` is the identity-provider subject
/// (mapped to an internal user id downstream); `email` is the trusted email
/// source for signup (never taken from request bodies).
#[derive(Debug, Deserialize, Serialize)]
pub struct JwtClaims {
    pub sub: String,
    pub exp: usize,
    pub iss: Option<String>,
    pub aud: Option<String>,
    pub role: Option<String>,
    pub email: Option<String>,
}

/// Verifies bearer tokens against the trusted key sources. Cheap to clone
/// (shares one inner instance); inject once via `.data(..)`.
#[derive(Clone)]
pub struct JwtVerifier {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    /// Supabase JWKS endpoint, if configured (production real-user tokens).
    jwks_url: Option<String>,
    /// Statically-trusted keys (the test keypair's public half). Always trusted.
    static_keys: Vec<Jwk>,
    /// Cache of the last-fetched remote JWKS. Refetched on an unknown `kid`.
    remote_cache: RwLock<Vec<Jwk>>,
    /// Expected `aud` claim. Tokens must carry this audience. Supabase issues
    /// `authenticated` for logged-in users.
    expected_audience: String,
    /// Clock-skew tolerance (seconds) applied to `exp` validation.
    leeway_secs: u64,
}

/// Verification failure. Callers map this to a 401.
#[derive(Debug)]
pub struct AuthError(pub String);

impl JwtVerifier {
    /// Build from the environment:
    /// - `SUPABASE_JWKS_URL` — optional Supabase JWKS endpoint.
    /// - `AGON_STATIC_JWKS` — optional JSON JWK Set of always-trusted keys.
    /// - `AGON_DEVICE_JWKS` — optional JSON JWK Set trusting the device-pairing
    ///   signing key (see `DeviceTokenSigner`). Kept separate from
    ///   `AGON_STATIC_JWKS` (documented as test/local-only) since this one is
    ///   meant to be set in every real deployment that wants device pairing
    ///   (Garmin, ...) to work — merged into the same always-trusted set, so
    ///   verification itself needs no special-casing.
    /// - `AGON_JWT_AUDIENCE` — expected `aud` claim (default `authenticated`).
    ///
    /// Every key source is optional so a deployment can run with only Supabase
    /// (no test/device key), but at least one must be present or every token
    /// is rejected — we warn loudly in that case.
    pub fn from_env() -> Self {
        let jwks_url = std::env::var("SUPABASE_JWKS_URL")
            .ok()
            .filter(|s| !s.is_empty());

        let expected_audience =
            std::env::var("AGON_JWT_AUDIENCE").unwrap_or_else(|_| "authenticated".to_string());

        let mut static_keys = load_jwk_set_env("AGON_STATIC_JWKS");
        static_keys.extend(load_jwk_set_env("AGON_DEVICE_JWKS"));

        if jwks_url.is_none() && static_keys.is_empty() {
            warn!(
                "no JWT trust configured (set SUPABASE_JWKS_URL and/or AGON_STATIC_JWKS); \
                 all tokens will be rejected"
            );
        }

        Self {
            inner: Arc::new(Inner {
                http: reqwest::Client::new(),
                jwks_url,
                static_keys,
                remote_cache: RwLock::new(Vec::new()),
                expected_audience,
                leeway_secs: 60,
            }),
        }
    }

    /// Verify a bearer token and return its claims, or an error if no trusted key
    /// matches / the signature or claims are invalid.
    pub async fn verify(&self, token: &str) -> Result<JwtClaims, AuthError> {
        let header =
            decode_header(token).map_err(|e| AuthError(format!("bad token header: {e}")))?;
        let kid = header
            .kid
            .ok_or_else(|| AuthError("token has no `kid`".into()))?;

        // Static keys first (cheap, in-memory), then the cached remote set.
        if let Some(jwk) = self.find_static(&kid) {
            return self.decode_with(token, &jwk, header.alg);
        }
        if let Some(jwk) = self.find_remote_cached(&kid) {
            return self.decode_with(token, &jwk, header.alg);
        }

        // Unknown kid: refetch the remote JWKS once (handles key rotation) and retry.
        if self.refresh_remote().await?
            && let Some(jwk) = self.find_remote_cached(&kid)
        {
            return self.decode_with(token, &jwk, header.alg);
        }

        Err(AuthError(format!("no trusted key for kid `{kid}`")))
    }

    /// Decode + verify a token against one JWK, enforcing the algorithm
    /// allowlist, expiry (with leeway), and the expected audience.
    fn decode_with(&self, token: &str, jwk: &Jwk, alg: Algorithm) -> Result<JwtClaims, AuthError> {
        // Only asymmetric algorithms are accepted — reject `alg: none` and any
        // symmetric alg outright, so a token can never dictate a weaker scheme.
        if !matches!(alg, Algorithm::ES256 | Algorithm::RS256) {
            return Err(AuthError(format!("unsupported alg {alg:?}")));
        }
        let key = DecodingKey::from_jwk(jwk).map_err(|e| AuthError(format!("bad JWK: {e}")))?;

        let mut validation = Validation::new(alg);
        validation.algorithms = vec![alg];
        validation.leeway = self.inner.leeway_secs;
        // Enforce expiry and audience. `exp` and `aud` are added to the required
        // claims so a token missing either is rejected, not silently accepted.
        validation.validate_exp = true;
        validation.validate_nbf = true;
        validation.validate_aud = true;
        validation.set_audience(&[&self.inner.expected_audience]);
        validation.set_required_spec_claims(&["exp", "aud", "sub"]);

        decode::<JwtClaims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|e| AuthError(format!("invalid token: {e}")))
    }

    fn find_static(&self, kid: &str) -> Option<Jwk> {
        find_by_kid(&self.inner.static_keys, kid)
    }

    fn find_remote_cached(&self, kid: &str) -> Option<Jwk> {
        let cache = self.inner.remote_cache.read().expect("jwks cache poisoned");
        find_by_kid(&cache, kid)
    }

    /// Fetch the Supabase JWKS and replace the cache. Returns whether a fetch
    /// actually happened (false if no URL is configured).
    async fn refresh_remote(&self) -> Result<bool, AuthError> {
        let Some(url) = &self.inner.jwks_url else {
            return Ok(false);
        };
        info!("refreshing remote JWKS from {url}");
        let set: JwkSet = self
            .inner
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AuthError(format!("fetch JWKS: {e}")))?
            .json()
            .await
            .map_err(|e| AuthError(format!("parse JWKS: {e}")))?;
        *self
            .inner
            .remote_cache
            .write()
            .expect("jwks cache poisoned") = set.keys;
        Ok(true)
    }
}

fn find_by_kid(keys: &[Jwk], kid: &str) -> Option<Jwk> {
    keys.iter()
        .find(|k| k.common.key_id.as_deref() == Some(kid))
        .cloned()
}

/// Load a JSON JWK Set from an env var, tolerating it being unset/empty (an
/// empty `Vec`) and logging (not failing) a malformed value — same "opt-in,
/// never fatal" treatment `AGON_STATIC_JWKS` has always had.
fn load_jwk_set_env(var: &str) -> Vec<Jwk> {
    match std::env::var(var) {
        Ok(raw) if !raw.is_empty() => match serde_json::from_str::<JwkSet>(&raw) {
            Ok(set) => set.keys,
            Err(e) => {
                warn!("{var} is not a valid JWK Set: {e}; ignoring");
                Vec::new()
            }
        },
        _ => Vec::new(),
    }
}

/// Mints device-credential JWTs during pairing (`POST /devices/pair` — see
/// `agon_core::dao::device_pairing`). A dedicated signing key, not the
/// Supabase-issued kind: the device never authenticates with Supabase at
/// all, so this is the sole source of trust for its long-lived token. Its
/// public half must be trusted by every `JwtVerifier` in the deployment via
/// `AGON_DEVICE_JWKS`, or a token minted here would verify nowhere.
///
/// **Known v1 limitation**: a minted token is a full, undifferentiated
/// account credential — `JwtClaims` carries no scope/device marker, so
/// `require_uid` (and everything built on it) treats a paired device
/// exactly like a normal login. A lost/compromised watch can therefore do
/// anything the account owner can, not just append live-scoring events, and
/// there's no revocation endpoint yet (deleting the device's `AUTH#<sub>`
/// guard item by hand is the only way to cut it off). Scoping this
/// properly — a `scope` claim plus a check in every sensitive handler — is
/// tracked as follow-up work; see `docs/garmin-live-scoring.md`.
#[derive(Clone)]
pub struct DeviceTokenSigner {
    private_key_pem: String,
    kid: String,
    audience: String,
}

/// How long a minted device token is valid. Long — there's no refresh flow
/// yet, and a watch has no practical way to re-pair itself unattended, so
/// the token needs to comfortably outlive a season rather than expire
/// underfoot mid-match.
pub const DEVICE_TOKEN_TTL: chrono::Duration = chrono::Duration::days(365);

impl DeviceTokenSigner {
    /// Reads `AGON_DEVICE_JWT_PRIVATE_KEY` (PEM, ES256 — matching the public
    /// key trusted via `AGON_DEVICE_JWKS`) and `AGON_DEVICE_JWT_KID` (default
    /// `agon-device`); `AGON_JWT_AUDIENCE` is shared with `JwtVerifier`.
    /// Returns `None` when the private key isn't set — device pairing is
    /// simply unavailable on this deployment, not a startup failure (same
    /// "absent means the feature is off" pattern as `assets::CloudFrontSigner`).
    pub fn from_env() -> Option<Self> {
        let private_key_pem = std::env::var("AGON_DEVICE_JWT_PRIVATE_KEY")
            .ok()
            .filter(|s| !s.is_empty())?;
        let kid =
            std::env::var("AGON_DEVICE_JWT_KID").unwrap_or_else(|_| "agon-device".to_string());
        let audience =
            std::env::var("AGON_JWT_AUDIENCE").unwrap_or_else(|_| "authenticated".to_string());
        Some(Self {
            private_key_pem,
            kid,
            audience,
        })
    }

    /// Mint a device token for `sub` (the device's own reserved auth
    /// identity — see `DevicePairingRecord::device_sub`), valid for
    /// [`DEVICE_TOKEN_TTL`].
    pub fn mint(&self, sub: &str) -> Result<String, AuthError> {
        let claims = JwtClaims {
            sub: sub.to_string(),
            exp: (chrono::Utc::now() + DEVICE_TOKEN_TTL).timestamp() as usize,
            iss: Some("agon-device".into()),
            aud: Some(self.audience.clone()),
            role: None,
            email: None,
        };

        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.kid.clone());

        let key = EncodingKey::from_ec_pem(self.private_key_pem.as_bytes())
            .map_err(|e| AuthError(format!("invalid AGON_DEVICE_JWT_PRIVATE_KEY: {e}")))?;

        encode(&header, &claims, &key)
            .map_err(|e| AuthError(format!("failed to sign device token: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    // A throwaway ES256 (P-256) keypair generated only for these tests. The JWK
    // is the public half of the PEM below; `kid` ties them together.
    // PKCS#8 PEM (`from_ec_pem` requires PKCS#8, not SEC1).
    const TEST_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgjkDc4ep8cMPLMPcg\n\
uxuqun2gyIcAExkBa3ftFbZYe4ShRANCAAQF8AGIa9WK61lduEn3imE8PFCJKzHy\n\
yuPC5L8ZcNr/wsPZHrn9SKPfMfhIiE9Ay0nj+7bSFLz3QafZDk6t6fbR\n\
-----END PRIVATE KEY-----\n";

    const TEST_JWKS: &str = r#"{"keys":[{"kty":"EC","crv":"P-256","alg":"ES256","use":"sig","kid":"agon-test","x":"BfABiGvViutZXbhJ94phPDxQiSsx8srjwuS_GXDa_8I","y":"w9keuf1Io98x-EiIT0DLSeP7ttIUvPdBp9kOTq3p9tE"}]}"#;

    fn static_verifier() -> JwtVerifier {
        let keys = serde_json::from_str::<JwkSet>(TEST_JWKS).unwrap().keys;
        JwtVerifier {
            inner: Arc::new(Inner {
                http: reqwest::Client::new(),
                jwks_url: None,
                static_keys: keys,
                remote_cache: RwLock::new(Vec::new()),
                expected_audience: "authenticated".into(),
                leeway_secs: 60,
            }),
        }
    }

    /// A far-future / far-past unix timestamp for exp tests.
    const FUTURE: usize = 4_102_444_800; // 2100-01-01
    const PAST: usize = 1_000_000_000; // 2001-09-09

    fn claims(exp: usize, aud: Option<&str>) -> JwtClaims {
        JwtClaims {
            sub: "user-1".into(),
            exp,
            iss: None,
            aud: aud.map(str::to_string),
            role: None,
            email: Some("user-1@example.com".into()),
        }
    }

    fn sign_with(claims: &JwtClaims, kid: Option<&str>, alg: Algorithm) -> String {
        let mut header = Header::new(alg);
        header.kid = kid.map(str::to_string);
        encode(
            &header,
            claims,
            &EncodingKey::from_ec_pem(TEST_PRIV_PEM.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    /// A valid token: trusted kid, ES256, future expiry, expected audience.
    fn valid_token() -> String {
        sign_with(
            &claims(FUTURE, Some("authenticated")),
            Some("agon-test"),
            Algorithm::ES256,
        )
    }

    #[tokio::test]
    async fn verifies_valid_token() {
        let out = static_verifier()
            .verify(&valid_token())
            .await
            .expect("should verify");
        assert_eq!(out.sub, "user-1");
        assert_eq!(out.email.as_deref(), Some("user-1@example.com"));
    }

    #[tokio::test]
    async fn rejects_unknown_kid() {
        let token = sign_with(
            &claims(FUTURE, Some("authenticated")),
            Some("nope"),
            Algorithm::ES256,
        );
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[tokio::test]
    async fn rejects_token_without_kid() {
        let token = sign_with(
            &claims(FUTURE, Some("authenticated")),
            None,
            Algorithm::ES256,
        );
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[tokio::test]
    async fn rejects_expired_token() {
        let token = sign_with(
            &claims(PAST, Some("authenticated")),
            Some("agon-test"),
            Algorithm::ES256,
        );
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[tokio::test]
    async fn rejects_wrong_audience() {
        let token = sign_with(
            &claims(FUTURE, Some("anon")),
            Some("agon-test"),
            Algorithm::ES256,
        );
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[tokio::test]
    async fn rejects_missing_audience() {
        let token = sign_with(&claims(FUTURE, None), Some("agon-test"), Algorithm::ES256);
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[tokio::test]
    async fn rejects_hs256_token() {
        // A symmetric-signed token must never be accepted, even if it names a
        // trusted kid — the alg allowlist rejects it.
        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("agon-test".into());
        let token = encode(
            &header,
            &claims(FUTURE, Some("authenticated")),
            &EncodingKey::from_secret(b"guessed"),
        )
        .unwrap();
        assert!(static_verifier().verify(&token).await.is_err());
    }

    // ── DeviceTokenSigner ───────────────────────────────────────────────

    // A second throwaway ES256 keypair standing in for the device-signing
    // key (`AGON_DEVICE_JWT_PRIVATE_KEY` / `AGON_DEVICE_JWKS`), generated only
    // for these tests — same spirit as TEST_PRIV_PEM/TEST_JWKS above, just a
    // distinct keypair so a device token can never accidentally verify
    // against the plain test-key trust store or vice versa.
    const DEVICE_PRIV_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg9aF8EoAP3W+9lWIp\n\
WZh/XzzyP7WuhO9mf3AC5JcWB1ehRANCAAT4Unn5HgobZXWGBnsnrn1h8E975gnU\n\
rdMKmVzU3xhtCEnDnDWxqRISC9Ylad533olIZCkeALgwXkkDIW+V1ePB\n\
-----END PRIVATE KEY-----\n";
    const DEVICE_KID: &str = "agon-device-dev";
    const DEVICE_JWKS: &str = r#"{"keys":[{"kty":"EC","crv":"P-256","alg":"ES256","use":"sig","kid":"agon-device-dev","x":"-FJ5-R4KG2V1hgZ7J659YfBPe-YJ1K3TCplc1N8YbQg","y":"ScOcNbGpEhIL1iVp3nfeiUhkKR4AuDBeSQMhb5XV48E"}]}"#;

    fn device_signer() -> DeviceTokenSigner {
        DeviceTokenSigner {
            private_key_pem: DEVICE_PRIV_PEM.to_string(),
            kid: DEVICE_KID.to_string(),
            audience: "authenticated".to_string(),
        }
    }

    /// A verifier trusting only the device-signing key — standing in for
    /// what `AGON_DEVICE_JWKS` wires into the real `static_keys` set.
    fn verifier_trusting_device_key() -> JwtVerifier {
        let keys = serde_json::from_str::<JwkSet>(DEVICE_JWKS).unwrap().keys;
        JwtVerifier {
            inner: Arc::new(Inner {
                http: reqwest::Client::new(),
                jwks_url: None,
                static_keys: keys,
                remote_cache: RwLock::new(Vec::new()),
                expected_audience: "authenticated".into(),
                leeway_secs: 60,
            }),
        }
    }

    #[tokio::test]
    async fn device_token_round_trips_through_the_verifier() {
        let token = device_signer().mint("device:abc123").expect("mint");

        let claims = verifier_trusting_device_key()
            .verify(&token)
            .await
            .expect("device token should verify against its own trusted key");

        assert_eq!(claims.sub, "device:abc123");
        assert_eq!(claims.aud.as_deref(), Some("authenticated"));
        // Long-lived, not a near-term expiry — see DEVICE_TOKEN_TTL.
        assert!(claims.exp as i64 > chrono::Utc::now().timestamp() + 60 * 60 * 24 * 300);
    }

    #[tokio::test]
    async fn device_token_does_not_verify_against_the_unrelated_test_key() {
        let token = device_signer().mint("device:abc123").expect("mint");
        assert!(static_verifier().verify(&token).await.is_err());
    }

    #[test]
    fn device_signer_from_env_is_none_when_unconfigured() {
        // Env var mutation is `unsafe` (edition 2024) since it's
        // process-global — fine here, nothing else in this crate reads this
        // particular var, and clearing it first makes the test correct even
        // if a real deployment config happens to be present in the process
        // environment.
        unsafe {
            std::env::remove_var("AGON_DEVICE_JWT_PRIVATE_KEY");
        }
        assert!(DeviceTokenSigner::from_env().is_none());
    }
}
