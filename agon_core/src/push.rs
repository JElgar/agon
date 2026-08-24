//! FCM HTTP v1 client, built on `google-fcm1` (auto-generated from Google's
//! own `fcm:v1` API schema) and `yup-oauth2` for the service-account OAuth2
//! flow, rather than a hand-rolled `reqwest` client: the generated types
//! track FCM's schema (including per-platform payload options) for free, and
//! `yup-oauth2` owns JWT signing + access-token caching/refresh internally.
//!
//! Shared by the worker (which sends a push whenever a `NotificationRecord`
//! is created — see `agon_worker/src/handlers/push.rs`).

use std::collections::HashMap;

use google_fcm1::api::{Message, SendMessageRequest};
use google_fcm1::hyper_rustls::{self, HttpsConnector};
use google_fcm1::hyper_util::client::legacy::connect::HttpConnector;
use google_fcm1::hyper_util::{self, client::legacy::Client as HyperClient};
use google_fcm1::yup_oauth2::{self, ServiceAccountAuthenticator};
use google_fcm1::{Error as FcmError, FirebaseCloudMessaging};

use crate::error::PushError;

pub type PushResult<T> = Result<T, PushError>;

/// Outcome of a single send. `Stale` means FCM rejected the token itself
/// (unregistered / not found) — the caller should delete the device row so it
/// stops being retried; any other failure is a [`PushError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    Sent,
    Stale,
}

/// A FCM HTTP v1 client. Cheap to clone — `FirebaseCloudMessaging` wraps an
/// `Arc`-backed hyper client and authenticator internally.
#[derive(Clone)]
pub struct PushClient {
    hub: FirebaseCloudMessaging<HttpsConnector<HttpConnector>>,
    project_id: String,
}

impl PushClient {
    /// Build a client from a GCP service-account key, as downloaded JSON
    /// (verbatim — this is what `AGON_FCM_SERVICE_ACCOUNT_JSON` holds).
    pub async fn new(service_account_json: &str) -> PushResult<Self> {
        let key = yup_oauth2::parse_service_account_key(service_account_json)
            .map_err(|e| PushError(format!("invalid service account key: {e}")))?;
        let project_id = key
            .project_id
            .clone()
            .ok_or_else(|| PushError("service account key missing project_id".into()))?;

        let auth = ServiceAccountAuthenticator::builder(key)
            .build()
            .await
            .map_err(|e| PushError(format!("failed to build authenticator: {e}")))?;

        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|e| PushError(format!("failed to load TLS roots: {e}")))?
            .https_only()
            .enable_http2()
            .build();
        let client = HyperClient::builder(hyper_util::rt::TokioExecutor::new()).build(connector);

        Ok(Self {
            hub: FirebaseCloudMessaging::new(client, auth),
            project_id,
        })
    }

    /// Send one push notification to a device's registration token, optionally
    /// carrying a `link` to open (a full URL — see `agon_worker::handlers::push`
    /// for how it's built) when the notification is clicked.
    ///
    /// Deliberately sent as a **data-only** message (everything — including
    /// title/body — in `data`, `notification` left unset) rather than FCM's
    /// `notification` field: on web, a message with a `notification` payload
    /// is auto-displayed by the browser using its own default handling
    /// whenever the page isn't in the foreground, which bypasses our service
    /// worker's `onBackgroundMessage` entirely — so a click on it can't carry
    /// our `link` anywhere. Data-only messages always reach
    /// `onBackgroundMessage`, giving us full control over both the display
    /// and the click behavior. See public/firebase-messaging-sw.js.
    pub async fn send(
        &self,
        push_token: &str,
        title: &str,
        body: &str,
        link: Option<&str>,
    ) -> PushResult<PushOutcome> {
        let mut data = HashMap::from([
            ("title".to_string(), title.to_string()),
            ("body".to_string(), body.to_string()),
        ]);
        if let Some(link) = link {
            data.insert("link".to_string(), link.to_string());
        }

        let request = SendMessageRequest {
            message: Some(Message {
                token: Some(push_token.to_string()),
                data: Some(data),
                ..Default::default()
            }),
            validate_only: None,
        };

        let parent = format!("projects/{}", self.project_id);
        match self
            .hub
            .projects()
            .messages_send(request, &parent)
            .doit()
            .await
        {
            Ok(_) => Ok(PushOutcome::Sent),
            Err(FcmError::BadRequest(body)) if is_stale_token_error(&body) => {
                Ok(PushOutcome::Stale)
            }
            Err(e) => Err(PushError(format!("send failed: {e}"))),
        }
    }
}

/// FCM's HTTP v1 error contract nests the platform-specific code under
/// `error.details[].errorCode`; a plain 404/`NOT_FOUND` is the other shape a
/// stale/unregistered token can show up as.
/// See https://firebase.google.com/docs/reference/fcm/rest/v1/ErrorCode.
fn is_stale_token_error(body: &serde_json::Value) -> bool {
    let error = &body["error"];
    error["status"] == "NOT_FOUND"
        || error["details"]
            .as_array()
            .is_some_and(|details| details.iter().any(|d| d["errorCode"] == "UNREGISTERED"))
}
