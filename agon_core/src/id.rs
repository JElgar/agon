//! Opaque id generation, shared by anything that mints a fresh entity id —
//! `agon_service`'s handlers and `agon_worker`'s `generate_series_occurrence`
//! activity alike. Lives here (rather than duplicated in each binary) because
//! the latter needed it too once series-occurrence generation moved off a
//! derived id onto an ordinary random one, guarded instead by the claim item
//! in `dao::series_ops` (see that module's doc comment).

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;

/// Generate a new opaque id: base64url-no-pad of 16 random bytes.
///
/// NO_PAD: the URL-safe alphabet (`-`/`_`) is fine, but trailing `=` padding
/// is not — these ids are used verbatim as Meilisearch document ids, and
/// Meili rejects `=` (only alphanumerics, `-`, `_` are allowed).
pub fn new_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    BASE64_URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_id_has_no_padding_and_is_base64url() {
        let id = new_id();
        assert!(!id.contains('='), "{id} should have no padding");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{id} should be base64url"
        );
    }

    #[test]
    fn new_id_is_not_constant() {
        // Not a rigorous randomness test — just a guard against an
        // accidentally-fixed seed/output.
        let ids: std::collections::HashSet<String> = (0..20).map(|_| new_id()).collect();
        assert_eq!(ids.len(), 20);
    }
}
