//! "Connect YouTube": a standalone Google OAuth2 flow granting YouTube
//! access, independent of how someone logs into Agon (Supabase email/
//! password today — see the design doc, "Google sign-in and the YouTube
//! scope"). Its own `OpenApi`-bearing type (`YoutubeApi`), combined with
//! the rest of the service's `Api` via `OpenApiService::new((Api,
//! admin::AdminApi, YoutubeApi), ...)` in `main.rs`, the same way
//! `admin::AdminApi` was pulled out.
//!
//! Flow: `agon_ui` opens `connect_url`'s URL in a popup/redirect; Google
//! bounces the user back to a page in `agon_ui` with a `code` query param;
//! that page calls `POST /youtube/connect` with the code, which is this
//! module's only handler that talks to Google's token endpoint. Nothing
//! here ever sees the user's Agon password or vice versa.
//!
//! What's NOT built yet (Phase 2, native-app-gated — see the design doc):
//! creating a live broadcast with the stored refresh token. This PR is
//! just the connection itself.

use openssl::rand::rand_bytes;
#[cfg(test)]
use openssl::symm::decrypt_aead;
use openssl::symm::{Cipher, encrypt_aead};
use poem::web::Data;
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, Object, OpenApi};
use serde::Deserialize;

use agon_services_core::dao;
use agon_services_core::dao::records::YoutubeConnectionRecord;

use crate::AuthSchema;
use crate::mapping::dao_internal;
use crate::require_uid;

pub struct YoutubeApi;

const YOUTUBE_SCOPE: &str = "https://www.googleapis.com/auth/youtube";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const YOUTUBE_CHANNELS_URL: &str =
    "https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true";

/// Google OAuth client config, loaded once at startup. `None` (via
/// `from_env`) means YouTube connection isn't configured on this
/// deployment — every handler below reports that as 503 rather than
/// failing confusingly partway through a flow, the same convention
/// `auth::DeviceTokenSigner::from_env` uses for device pairing.
#[derive(Clone)]
pub struct GoogleOAuthConfig {
    client_id: String,
    client_secret: String,
    /// Where Google redirects back to after consent — an `agon_ui` page,
    /// not this service (this service never receives Google's redirect
    /// directly; `agon_ui` reads the `code` off its own URL and calls
    /// `POST /youtube/connect` with it).
    redirect_uri: String,
    http: reqwest::Client,
}

impl GoogleOAuthConfig {
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").ok()?;
        let redirect_uri = std::env::var("GOOGLE_OAUTH_REDIRECT_URI").ok()?;
        Some(Self {
            client_id,
            client_secret,
            redirect_uri,
            http: reqwest::Client::new(),
        })
    }

    fn consent_url(&self) -> String {
        url::Url::parse_with_params(
            "https://accounts.google.com/o/oauth2/v2/auth",
            &[
                ("client_id", self.client_id.as_str()),
                ("redirect_uri", self.redirect_uri.as_str()),
                ("response_type", "code"),
                ("scope", YOUTUBE_SCOPE),
                ("access_type", "offline"),
                ("prompt", "consent"),
                ("include_granted_scopes", "true"),
            ],
        )
        .expect("static base URL always parses")
        .to_string()
    }
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    scope: String,
}

#[derive(Deserialize)]
struct YoutubeChannelsResponse {
    items: Vec<YoutubeChannelItem>,
}

#[derive(Deserialize)]
struct YoutubeChannelItem {
    id: String,
    snippet: YoutubeChannelSnippet,
}

#[derive(Deserialize)]
struct YoutubeChannelSnippet {
    title: String,
}

/// Encrypts/decrypts refresh tokens at rest (AES-256-GCM). The DAO stores
/// only what this produces — an opaque base64 blob — and never sees the
/// key or the plaintext. `None` (via `from_env`) means YouTube connection
/// isn't configured, same as `GoogleOAuthConfig`.
#[derive(Clone)]
pub struct TokenCipher {
    key: Vec<u8>,
}

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

impl TokenCipher {
    /// Reads `AGON_YOUTUBE_TOKEN_KEY`: base64, must decode to exactly 32
    /// bytes (AES-256's key size). Deliberately `None`, not a panic, on a
    /// missing/malformed key — a misconfigured deployment should report
    /// "not configured" (503) rather than fail to start.
    pub fn from_env() -> Option<Self> {
        let encoded = std::env::var("AGON_YOUTUBE_TOKEN_KEY").ok()?;
        let key =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).ok()?;
        if key.len() != 32 {
            return None;
        }
        Some(Self { key })
    }

    /// Encrypts `plaintext`, returning `base64(nonce || tag || ciphertext)`.
    /// A fresh random nonce every call (GCM requires never reusing a nonce
    /// under the same key) — prepended rather than stored separately so
    /// the DAO's `encrypted_refresh_token` stays a single opaque string.
    fn encrypt(&self, plaintext: &str) -> Result<String, openssl::error::ErrorStack> {
        let mut nonce = [0u8; NONCE_LEN];
        rand_bytes(&mut nonce)?;
        let mut tag = [0u8; TAG_LEN];
        let ciphertext = encrypt_aead(
            Cipher::aes_256_gcm(),
            &self.key,
            Some(&nonce),
            &[],
            plaintext.as_bytes(),
            &mut tag,
        )?;
        let mut blob = Vec::with_capacity(NONCE_LEN + TAG_LEN + ciphertext.len());
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&tag);
        blob.extend_from_slice(&ciphertext);
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            blob,
        ))
    }

    #[cfg(test)]
    fn decrypt(&self, blob: &str) -> Option<String> {
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob).ok()?;
        if bytes.len() < NONCE_LEN + TAG_LEN {
            return None;
        }
        let (nonce, rest) = bytes.split_at(NONCE_LEN);
        let (tag, ciphertext) = rest.split_at(TAG_LEN);
        let plaintext = decrypt_aead(
            Cipher::aes_256_gcm(),
            &self.key,
            Some(nonce),
            &[],
            ciphertext,
            tag,
        )
        .ok()?;
        String::from_utf8(plaintext).ok()
    }
}

#[derive(Object)]
struct YoutubeConnectUrl {
    url: String,
}

#[derive(Object)]
struct YoutubeConnectInput {
    /// The `code` query param Google redirected `agon_ui` back with.
    code: String,
}

#[derive(Object)]
struct YoutubeConnectionStatus {
    connected: bool,
    channel_title: Option<String>,
}

#[derive(ApiResponse)]
enum YoutubeConnectUrlResponse {
    #[oai(status = 200)]
    Ok(Json<YoutubeConnectUrl>),

    /// YouTube connection isn't configured on this deployment (missing
    /// `GOOGLE_OAUTH_*` env vars — see `GoogleOAuthConfig::from_env`).
    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum YoutubeConnectResponse {
    #[oai(status = 200)]
    Ok(Json<YoutubeConnectionStatus>),

    /// Google didn't return a refresh token — happens if the user has
    /// already granted this app offline access before and Google's consent
    /// screen was skipped. The client should send the user back through
    /// Google's "remove access" flow for this app, then retry.
    #[oai(status = 400)]
    NoRefreshToken(PlainText<String>),

    /// The code exchange or the channel lookup failed against Google's API.
    #[oai(status = 502)]
    GoogleError(PlainText<String>),

    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum YoutubeConnectionStatusResponse {
    #[oai(status = 200)]
    Ok(Json<YoutubeConnectionStatus>),
}

#[OpenApi]
impl YoutubeApi {
    /// The Google consent URL to send the user to. Doesn't itself require
    /// auth beyond being a logged-in Agon user — the uid isn't embedded in
    /// this URL at all; `POST /youtube/connect` resolves it fresh from the
    /// caller's own bearer token.
    #[oai(path = "/youtube/connect-url", method = "get")]
    async fn youtube_connect_url(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(oauth): Data<&Option<GoogleOAuthConfig>>,
        AuthSchema(jwt): AuthSchema,
    ) -> poem::Result<YoutubeConnectUrlResponse> {
        require_uid(dao, &jwt).await?;
        let Some(oauth) = oauth else {
            return Ok(YoutubeConnectUrlResponse::NotConfigured(PlainText(
                "YouTube connection is not configured on this deployment".into(),
            )));
        };
        Ok(YoutubeConnectUrlResponse::Ok(Json(YoutubeConnectUrl {
            url: oauth.consent_url(),
        })))
    }

    /// Exchange an OAuth code for tokens, look up the connected channel,
    /// and store the (encrypted) refresh token against the caller.
    /// Reconnecting (e.g. a different Google account) overwrites the
    /// previous connection.
    #[oai(path = "/youtube/connect", method = "post")]
    async fn youtube_connect(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(oauth): Data<&Option<GoogleOAuthConfig>>,
        Data(cipher): Data<&Option<TokenCipher>>,
        AuthSchema(jwt): AuthSchema,
        Json(input): Json<YoutubeConnectInput>,
    ) -> poem::Result<YoutubeConnectResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let (Some(oauth), Some(cipher)) = (oauth, cipher) else {
            return Ok(YoutubeConnectResponse::NotConfigured(PlainText(
                "YouTube connection is not configured on this deployment".into(),
            )));
        };

        let token_res = oauth
            .http
            .post(GOOGLE_TOKEN_URL)
            .form(&[
                ("code", input.code.as_str()),
                ("client_id", oauth.client_id.as_str()),
                ("client_secret", oauth.client_secret.as_str()),
                ("redirect_uri", oauth.redirect_uri.as_str()),
                ("grant_type", "authorization_code"),
            ])
            .send()
            .await
            .map_err(google_error)?;
        if !token_res.status().is_success() {
            let body = token_res.text().await.unwrap_or_default();
            return Ok(YoutubeConnectResponse::GoogleError(PlainText(format!(
                "token exchange failed: {body}"
            ))));
        }
        let token: GoogleTokenResponse = token_res.json().await.map_err(google_error)?;
        let Some(refresh_token) = token.refresh_token else {
            return Ok(YoutubeConnectResponse::NoRefreshToken(PlainText(
                "Google did not return a refresh token \u{2014} remove this app's access in your \
                 Google account and try connecting again"
                    .into(),
            )));
        };

        let channels_res = oauth
            .http
            .get(YOUTUBE_CHANNELS_URL)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(google_error)?;
        if !channels_res.status().is_success() {
            let body = channels_res.text().await.unwrap_or_default();
            return Ok(YoutubeConnectResponse::GoogleError(PlainText(format!(
                "channel lookup failed: {body}"
            ))));
        }
        let channels: YoutubeChannelsResponse = channels_res.json().await.map_err(google_error)?;
        let Some(channel) = channels.items.into_iter().next() else {
            return Ok(YoutubeConnectResponse::GoogleError(PlainText(
                "no YouTube channel found on this Google account".into(),
            )));
        };

        let encrypted_refresh_token = cipher.encrypt(&refresh_token).map_err(|e| {
            poem::Error::from_string(
                format!("failed to encrypt refresh token: {e}"),
                poem::http::StatusCode::INTERNAL_SERVER_ERROR,
            )
        })?;

        let record = YoutubeConnectionRecord {
            user_id: uid,
            channel_id: channel.id,
            channel_title: channel.snippet.title.clone(),
            encrypted_refresh_token,
            scope: token.scope,
            connected_at: chrono::Utc::now().to_rfc3339(),
        };
        dao.upsert_youtube_connection(&record)
            .await
            .map_err(dao_internal)?;

        Ok(YoutubeConnectResponse::Ok(Json(YoutubeConnectionStatus {
            connected: true,
            channel_title: Some(channel.snippet.title),
        })))
    }

    /// Whether the caller has a YouTube connection, and which channel —
    /// for the settings page's "Connected as {channel}" UI.
    #[oai(path = "/youtube/connection", method = "get")]
    async fn youtube_connection(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
    ) -> poem::Result<YoutubeConnectionStatusResponse> {
        let uid = require_uid(dao, &jwt).await?;
        let connection = dao
            .get_youtube_connection(&uid)
            .await
            .map_err(dao_internal)?;
        Ok(YoutubeConnectionStatusResponse::Ok(Json(
            YoutubeConnectionStatus {
                connected: connection.is_some(),
                channel_title: connection.map(|c| c.channel_title),
            },
        )))
    }

    /// Disconnect YouTube. Doesn't revoke the grant on Google's side (the
    /// user can do that from their Google account) — just stops Agon from
    /// being able to use it.
    #[oai(path = "/youtube/connection", method = "delete")]
    async fn youtube_disconnect(
        &self,
        Data(dao): Data<&dao::Dao>,
        AuthSchema(jwt): AuthSchema,
    ) -> poem::Result<PlainText<String>> {
        let uid = require_uid(dao, &jwt).await?;
        dao.delete_youtube_connection(&uid)
            .await
            .map_err(dao_internal)?;
        Ok(PlainText("disconnected".into()))
    }
}

fn google_error(e: reqwest::Error) -> poem::Error {
    poem::Error::from_string(
        format!("request to Google failed: {e}"),
        poem::http::StatusCode::BAD_GATEWAY,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> TokenCipher {
        TokenCipher { key: vec![7u8; 32] }
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let c = cipher();
        let blob = c.encrypt("a-real-refresh-token").unwrap();
        assert_eq!(c.decrypt(&blob).unwrap(), "a-real-refresh-token");
    }

    #[test]
    fn two_encryptions_of_the_same_plaintext_differ() {
        // Different random nonces each call -> different ciphertext, even
        // for the same input. A non-random/reused nonce would be a real
        // vulnerability for GCM, so this is worth pinning down in a test.
        let c = cipher();
        assert_ne!(
            c.encrypt("same-token").unwrap(),
            c.encrypt("same-token").unwrap()
        );
    }

    #[test]
    fn tampered_ciphertext_fails_to_decrypt() {
        let c = cipher();
        let mut blob = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            c.encrypt("a-real-refresh-token").unwrap(),
        )
        .unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        let tampered = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, blob);
        assert!(c.decrypt(&tampered).is_none());
    }

    #[test]
    fn from_env_is_none_when_unconfigured() {
        // SAFETY: test-only, no other test in this process reads this var
        // concurrently.
        unsafe {
            std::env::remove_var("AGON_YOUTUBE_TOKEN_KEY");
        }
        assert!(TokenCipher::from_env().is_none());
    }
}
