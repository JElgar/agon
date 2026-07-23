//! Minimal FCM HTTP v1 client.
//!
//! Shared by the worker (which sends a push whenever a `NotificationRecord` is
//! created — see `agon_worker/src/handlers/push.rs`). Talks to Firebase Cloud
//! Messaging directly with `reqwest` rather than pulling in a Google Cloud SDK,
//! matching `search::SearchClient`'s "thin REST client" approach: the surface
//! we need is one call (send to a token), so a full SDK buys nothing.
//!
//! Auth is the FCM HTTP v1 OAuth2 service-account flow (RFC 7523 JWT bearer):
//! a locally-signed, short-lived JWT is exchanged at Google's token endpoint
//! for an access token, which is cached until shortly before it expires.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::PushError;

pub type PushResult<T> = Result<T, PushError>;

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
/// Refresh this many seconds before the token's real expiry, so a send never
/// races an expiring-but-not-yet-expired cached token.
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

/// Outcome of a single send. `Stale` means FCM rejected the token itself
/// (unregistered / not found) — the caller should delete the device row so it
/// stops being retried; any other failure is a [`PushError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Sent,
    Stale,
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

/// A thin FCM HTTP v1 client. Cheap to clone (wraps an `Arc` internally, like
/// `SearchClient`); construct once per process.
#[derive(Clone)]
pub struct PushClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    project_id: String,
    service_account_email: String,
    signing_key: EncodingKey,
    cached_token: RwLock<Option<CachedToken>>,
}

#[derive(Serialize)]
struct TokenClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: i64,
    exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// The shape of a downloaded GCP service-account key JSON file — only the
/// fields `PushClient` needs.
#[derive(Deserialize)]
pub struct ServiceAccountJson {
    pub project_id: String,
    pub client_email: String,
    pub private_key: String,
}

#[derive(Serialize)]
struct SendRequest<'a> {
    message: Message<'a>,
}

#[derive(Serialize)]
struct Message<'a> {
    token: &'a str,
    notification: Notification<'a>,
}

#[derive(Serialize)]
struct Notification<'a> {
    title: &'a str,
    body: &'a str,
}

/// The subset of FCM's error response body we need to detect a stale token.
/// See https://firebase.google.com/docs/reference/fcm/rest/v1/ErrorCode.
#[derive(Deserialize, Default)]
struct FcmErrorBody {
    #[serde(default)]
    error: FcmError,
}

#[derive(Deserialize, Default)]
struct FcmError {
    #[serde(default)]
    status: String,
    #[serde(default)]
    details: Vec<FcmErrorDetail>,
}

#[derive(Deserialize, Default)]
struct FcmErrorDetail {
    #[serde(rename = "errorCode", default)]
    error_code: String,
}

impl PushClient {
    /// Build a client from a parsed service-account key.
    pub fn new(service_account: ServiceAccountJson) -> PushResult<Self> {
        let signing_key = EncodingKey::from_rsa_pem(service_account.private_key.as_bytes())
            .map_err(|e| PushError(format!("invalid service account private key: {e}")))?;
        Ok(Self {
            inner: Arc::new(Inner {
                http: reqwest::Client::new(),
                project_id: service_account.project_id,
                service_account_email: service_account.client_email,
                signing_key,
                cached_token: RwLock::new(None),
            }),
        })
    }

    /// A valid OAuth2 access token, refreshing it if absent or about to expire.
    async fn access_token(&self) -> PushResult<String> {
        {
            let cached = self.inner.cached_token.read().await;
            if let Some(t) = cached.as_ref()
                && Instant::now() < t.expires_at
            {
                return Ok(t.access_token.clone());
            }
        }

        let mut cached = self.inner.cached_token.write().await;
        // Another task may have refreshed it while we waited for the write lock.
        if let Some(t) = cached.as_ref()
            && Instant::now() < t.expires_at
        {
            return Ok(t.access_token.clone());
        }

        let now = chrono::Utc::now().timestamp();
        let claims = TokenClaims {
            iss: &self.inner.service_account_email,
            scope: FCM_SCOPE,
            aud: TOKEN_URL,
            iat: now,
            exp: now + 3600,
        };
        let assertion = encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &self.inner.signing_key,
        )
        .map_err(|e| PushError(format!("failed to sign service account JWT: {e}")))?;

        let resp = self
            .inner
            .http
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &assertion),
            ])
            .send()
            .await
            .map_err(|e| PushError(format!("token exchange request failed: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(PushError(format!(
                "token exchange returned {status}: {text}"
            )));
        }
        let token: TokenResponse = resp
            .json()
            .await
            .map_err(|e| PushError(format!("bad token response: {e}")))?;

        let expires_at =
            Instant::now() + Duration::from_secs(token.expires_in).saturating_sub(REFRESH_MARGIN);
        *cached = Some(CachedToken {
            access_token: token.access_token.clone(),
            expires_at,
        });
        Ok(token.access_token)
    }

    /// Send one push notification to a device's registration token.
    pub async fn send(&self, push_token: &str, title: &str, body: &str) -> PushResult<PushOutcome> {
        let access_token = self.access_token().await?;
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.inner.project_id
        );

        let resp = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&access_token)
            .json(&SendRequest {
                message: Message {
                    token: push_token,
                    notification: Notification { title, body },
                },
            })
            .send()
            .await
            .map_err(|e| PushError(format!("send request failed: {e}")))?;

        let status = resp.status();
        if status.is_success() {
            return Ok(PushOutcome::Sent);
        }

        let text = resp.text().await.unwrap_or_default();
        let parsed: FcmErrorBody = serde_json::from_str(&text).unwrap_or_default();
        let is_stale = status == reqwest::StatusCode::NOT_FOUND
            || parsed.error.status == "NOT_FOUND"
            || parsed
                .error
                .details
                .iter()
                .any(|d| d.error_code == "UNREGISTERED");
        if is_stale {
            return Ok(PushOutcome::Stale);
        }
        Err(PushError(format!("send returned {status}: {text}")))
    }
}
