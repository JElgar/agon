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
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
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
    /// - `AGON_JWT_AUDIENCE` — expected `aud` claim (default `authenticated`).
    ///
    /// Both key sources are optional so a deployment can run with only Supabase
    /// (no test key) or only the static set (local/offline), but at least one
    /// must be present or every token is rejected — we warn loudly in that case.
    pub fn from_env() -> Self {
        let jwks_url = std::env::var("SUPABASE_JWKS_URL")
            .ok()
            .filter(|s| !s.is_empty());

        let expected_audience =
            std::env::var("AGON_JWT_AUDIENCE").unwrap_or_else(|_| "authenticated".to_string());

        let static_keys = match std::env::var("AGON_STATIC_JWKS") {
            Ok(raw) if !raw.is_empty() => match serde_json::from_str::<JwkSet>(&raw) {
                Ok(set) => set.keys,
                Err(e) => {
                    warn!("AGON_STATIC_JWKS is not a valid JWK Set: {e}; ignoring");
                    Vec::new()
                }
            },
            _ => Vec::new(),
        };

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
}
