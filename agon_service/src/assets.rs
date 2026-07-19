//! Object-storage integration for uploadable assets (profile / team / match
//! images).
//!
//! Two halves, matching the asset lifecycle:
//!   - **Upload**: the client asks for an `Asset` (`POST /assets`), and we hand
//!     back a short-lived **S3 presigned PUT** it replays to upload the bytes
//!     directly to the private bucket. The API never proxies the bytes.
//!   - **Serve**: once the storage-event worker flips the asset to `Uploaded`,
//!     it stores the object's canonical CDN URL. Profile/team images are served
//!     through a *public* CloudFront behaviour (returned verbatim); match-header
//!     images are served through a *signed* behaviour, so the URL is minted with
//!     a short expiry at read time — this is where future per-match visibility
//!     (e.g. followers-only) is enforced, without changing the upload path.
//!
//! The bucket itself stays fully private (Origin Access Control): CloudFront is
//! the only reader. See `agon_infra/index.ts`.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aws_sdk_s3::presigning::PresigningConfig;
use openssl::hash::MessageDigest;
use openssl::pkey::{PKey, Private};
use openssl::sign::Signer;

use crate::{UploadHeader, UploadTarget};

/// How long a presigned upload URL is valid. Short — the client uploads
/// immediately after creating the asset, and re-reading the asset regenerates a
/// fresh target (the retry mechanism).
const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);

/// How long a signed match-header GET URL is valid. Long enough to render a feed
/// / detail page comfortably, short enough that a leaked URL expires.
const SIGNED_GET_TTL: Duration = Duration::from_secs(60 * 60);

/// CloudFront key-pair signer for private (signed-URL) serving. Present only when
/// the signing config is supplied; absent in local/dev, where signing falls back
/// to the plain CDN URL.
struct CloudFrontSigner {
    key_pair_id: String,
    private_key: PKey<Private>,
}

/// Storage integration handle: presigned PUT generation (S3) + serving-URL
/// construction (CloudFront public/signed). Cheap to clone (the S3 client is an
/// `Arc` internally); injected into handlers via poem `.data(..)`.
#[derive(Clone)]
pub struct Assets {
    s3: aws_sdk_s3::Client,
    bucket: String,
    /// CloudFront base URL, no trailing slash, e.g. `https://cdn.get-agon.com`.
    cdn_base: String,
    /// Present when CloudFront URL signing is configured (prod). `Arc` so `Assets`
    /// stays `Clone` even though `PKey` is not.
    signer: Option<std::sync::Arc<CloudFrontSigner>>,
}

impl Assets {
    /// Build from the ambient AWS config (same creds as the DAO) plus the asset
    /// env vars:
    ///   - `AGON_ASSETS_BUCKET` — the private S3 bucket (required).
    ///   - `AGON_ASSETS_CDN_URL` — CloudFront base URL for serving. When unset
    ///     (dev), serving URLs are relative to an empty base — only meaningful
    ///     once set in prod.
    ///   - `AGON_CLOUDFRONT_KEY_PAIR_ID` + `AGON_CLOUDFRONT_PRIVATE_KEY` (PEM) —
    ///     enable signed match-header URLs. Both must be present to sign;
    ///     otherwise serving falls back to the plain CDN URL (dev).
    pub async fn from_env() -> Self {
        let config = aws_config::load_from_env().await;
        let s3 = aws_sdk_s3::Client::new(&config);
        let bucket = std::env::var("AGON_ASSETS_BUCKET").unwrap_or_else(|_| "agon-assets".into());
        let cdn_base = std::env::var("AGON_ASSETS_CDN_URL")
            .unwrap_or_default()
            .trim_end_matches('/')
            .to_string();

        let signer = match (
            std::env::var("AGON_CLOUDFRONT_KEY_PAIR_ID").ok(),
            std::env::var("AGON_CLOUDFRONT_PRIVATE_KEY").ok(),
        ) {
            (Some(key_pair_id), Some(pem)) if !key_pair_id.is_empty() && !pem.is_empty() => {
                match PKey::private_key_from_pem(pem.as_bytes()) {
                    Ok(private_key) => Some(std::sync::Arc::new(CloudFrontSigner {
                        key_pair_id,
                        private_key,
                    })),
                    Err(e) => {
                        tracing::error!(error = %e, "invalid AGON_CLOUDFRONT_PRIVATE_KEY; match-header URLs will be unsigned");
                        None
                    }
                }
            }
            _ => {
                tracing::info!(
                    "CloudFront signing not configured; match-header URLs served unsigned"
                );
                None
            }
        };

        Self {
            s3,
            bucket,
            cdn_base,
            signer,
        }
    }

    /// Generate a short-lived presigned PUT the client replays to upload bytes.
    /// The `Content-Type` and exact `Content-Length` are baked into the signature,
    /// so the client must send the same header and a file of exactly that size —
    /// S3 rejects any mismatch. Provider-agnostic on the wire: the client just
    /// replays `method` + `headers` against `upload_url`. A non-positive
    /// `content_length` omits the constraint (legacy assets predating the field).
    pub async fn presign_put(
        &self,
        storage_key: &str,
        content_type: &str,
        content_length: i64,
    ) -> Result<UploadTarget, String> {
        let mut req = self
            .s3
            .put_object()
            .bucket(&self.bucket)
            .key(storage_key)
            .content_type(content_type);
        if content_length > 0 {
            req = req.content_length(content_length);
        }
        let presigned = req
            .presigned(
                PresigningConfig::expires_in(UPLOAD_URL_TTL)
                    .map_err(|e| format!("presign config: {e}"))?,
            )
            .await
            .map_err(|e| format!("presign put: {e}"))?;

        // Replay every header the SDK signed (at minimum host/content-type), so the
        // client's PUT matches the signature exactly.
        let headers = presigned
            .headers()
            .map(|(name, value)| UploadHeader {
                name: name.to_string(),
                value: value.to_string(),
            })
            .collect();

        Ok(UploadTarget {
            upload_url: presigned.uri().to_string(),
            method: String::from("PUT"),
            headers,
        })
    }

    /// The canonical, public CloudFront URL for an object. Used both by profile/
    /// team image serving (returned as-is) and as the base the match-header signer
    /// signs. `AGON_ASSETS_CDN_URL` must be set for this to be reachable.
    pub fn public_url(&self, storage_key: &str) -> String {
        format!("{}/{}", self.cdn_base, storage_key)
    }

    /// Sign an already-canonical CDN URL for private (match-header) serving. When
    /// no signer is configured (dev/local) the URL is returned unchanged, so the
    /// contract is identical — only the enforcement differs by environment.
    ///
    /// Uses a CloudFront **canned policy** (single resource, expiry only), which
    /// is RSA-SHA1 over the policy document, base64'd with CloudFront's URL-safe
    /// alphabet.
    pub fn sign_get(&self, url: &str) -> String {
        let Some(signer) = &self.signer else {
            return url.to_string();
        };
        match sign_canned(signer, url, SIGNED_GET_TTL) {
            Ok(signed) => signed,
            Err(e) => {
                // Never fail a read on a signing error — fall back to the plain
                // URL (which a private distribution will 403, surfacing the misconfig
                // without taking the endpoint down).
                tracing::error!(error = %e, "cloudfront signing failed; returning unsigned url");
                url.to_string()
            }
        }
    }
}

/// Build a CloudFront canned-policy signed URL.
fn sign_canned(signer: &CloudFrontSigner, url: &str, ttl: Duration) -> Result<String, String> {
    let expiry = (SystemTime::now() + ttl)
        .duration_since(UNIX_EPOCH)
        .map_err(|e| format!("clock: {e}"))?
        .as_secs();

    // Canned policy: exact resource + a DateLessThan expiry. Must be compact
    // (no whitespace) — the signature is over these exact bytes.
    let policy = format!(
        r#"{{"Statement":[{{"Resource":"{url}","Condition":{{"DateLessThan":{{"AWS:EpochTime":{expiry}}}}}}}]}}"#
    );

    let mut s = Signer::new(MessageDigest::sha1(), &signer.private_key)
        .map_err(|e| format!("signer init: {e}"))?;
    s.update(policy.as_bytes())
        .map_err(|e| format!("signer update: {e}"))?;
    let signature = s.sign_to_vec().map_err(|e| format!("sign: {e}"))?;

    let sig = cf_base64(&signature);
    let sep = if url.contains('?') { '&' } else { '?' };
    Ok(format!(
        "{url}{sep}Expires={expiry}&Signature={sig}&Key-Pair-Id={kpid}",
        kpid = signer.key_pair_id
    ))
}

/// CloudFront's URL-safe base64 variant: standard base64 with `+/=` replaced by
/// `-~_` respectively.
fn cf_base64(bytes: &[u8]) -> String {
    use base64::{Engine, engine::general_purpose::STANDARD};
    STANDARD
        .encode(bytes)
        .replace('+', "-")
        .replace('/', "~")
        .replace('=', "_")
}
