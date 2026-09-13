//! Renders the device-pairing QR code the Garmin watch app displays (see
//! `docs/garmin-live-scoring.md` and `agon_core::dao::device_pairing`) —
//! purely a rendering function, no database access at all: it doesn't even
//! know or care whether `code` is a real pairing code, only that the caller
//! wants a QR of some URL built from it. The pairing record only ever gets
//! created once someone actually visits that URL and confirms (see
//! `Api::confirm_device_pairing` in `main.rs`) — this endpoint runs before
//! any of that exists.
//!
//! A plain (non-OpenAPI) poem route, not a typed `#[oai(...)]` endpoint —
//! same reasoning as `share.rs`: PNG bytes with a `Content-Type: image/png`
//! header don't fit poem-openapi's JSON-oriented `ApiResponse` machinery
//! (its `Binary`/`Attachment` payload types are hardcoded to
//! `application/octet-stream`), and the watch's
//! `Communications.makeImageRequest` needs a real image content type to
//! decode this as one.

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder, Luma};
use poem::http::StatusCode;
use poem::web::{Data, Path};
use poem::{IntoResponse, Response, handler};
use qrcode::QrCode;

use crate::share::UiBaseUrl;

/// Real codes are always exactly 6 characters (see `new_pairing_code` on
/// the watch side, Monkey C) — this doesn't validate that shape at all
/// (this handler has no way to know what a "real" code even looks like),
/// just caps the input length generously so a wildly oversized query
/// string can't be handed straight to the QR encoder.
const MAX_CODE_LEN: usize = 64;

#[handler]
pub fn render_pairing_qr(
    Path(code): Path<String>,
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
) -> Response {
    if code.is_empty() || code.len() > MAX_CODE_LEN {
        return StatusCode::BAD_REQUEST.into_response();
    }

    // The web app's own pairing-confirmation page — see agon_ui's `/pair`
    // route. Not a real pairing record yet; just a URL.
    let pairing_url = format!("{ui_base_url}/pair?code={code}");

    match render_qr_png(&pairing_url) {
        Some(png_bytes) => Response::builder()
            .content_type("image/png")
            .body(png_bytes),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Encode `data` as a QR code and render it to PNG bytes. `None` on any
/// encoding failure (an oversized/invalid payload for a QR code — not
/// expected in practice given `MAX_CODE_LEN`, but this is the one place
/// that would ever see it).
fn render_qr_png(data: &str) -> Option<Vec<u8>> {
    let qr = QrCode::new(data.as_bytes()).ok()?;
    // 6x6 pixels per module: legible to a phone camera at arm's length
    // without producing a needlessly large PNG for the watch's own radio
    // (or a phone relaying it) to fetch.
    let image = qr.render::<Luma<u8>>().module_dimensions(6, 6).build();

    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::L8,
        )
        .ok()?;
    Some(png_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real PNG's magic bytes — this is what actually matters here:
    /// `Communications.makeImageRequest` on the watch needs genuine,
    /// decodable PNG bytes, not just "some bytes came back".
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    #[test]
    fn encodes_a_url_as_a_real_png() {
        let png = render_qr_png("https://agon.example.com/pair?code=ABC123").unwrap();
        assert!(png.starts_with(PNG_MAGIC));
    }

    #[test]
    fn empty_input_still_encodes_fine() {
        // QrCode::new accepts an empty payload — nothing here specifically
        // relies on that, just confirming this pure function doesn't
        // assume a minimum length the way the HTTP handler's own
        // MAX_CODE_LEN/emptiness check does.
        assert!(render_qr_png("").is_some());
    }
}
