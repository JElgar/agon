//! Match series operations: create, get, and the two guarded updates the
//! `materialize_series` workflow drives generation forward with
//! (`bump_series_occurrences_generated`, `update_series_status`). Actually
//! generating a series' occurrences into independent matches is not part of
//! this module — that's `agon_worker`'s `materialize_series` workflow, via
//! the ordinary `Dao::create_match` every other match creation path uses.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::AttributeValue;
use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::SeriesRecord;

pub const TYPE_SERIES: &str = "series";

/// Deterministically derive the match id for one occurrence of a series.
/// `index` is 0-based; index 0 is the series' first occurrence, which is
/// created synchronously by the API with an ordinary random `new_id()`, not
/// through this scheme — only `materialize_series` (indices 1..) uses it.
///
/// Pure function of `(series_id, index)`: the same pair always yields the
/// same id, so a retried `materialize_series` activity that calls
/// `create_match` again for an occurrence it (or an earlier attempt) already
/// created hits `Conflict` — handled as "already done" — instead of creating
/// a duplicate match. Plain random ids (`new_id()`) can't serve this role: a
/// retry needs to land on the *same* id it used last time, not a fresh one.
/// A thin, descriptively-named wrapper over [`deterministic_id`].
pub fn series_occurrence_id(series_id: &str, index: u32) -> String {
    deterministic_id(&format!("{series_id}-{index}"))
}

/// A deterministic, opaque id derived from `seed`: the same `seed` always
/// yields the same id. Used wherever something needs an id that survives a
/// retry landing on the same value rather than a fresh one every attempt —
/// [`series_occurrence_id`] is the current caller, keying `seed` off
/// `(series_id, index)`; `materialize_series`'s activities use this same
/// function directly (with their own seeds, e.g. a generated occurrence's
/// own side ids) for the same reason.
///
/// FNV-1a-64 over `seed`'s bytes, run twice with different offset bases to
/// get 128 bits, then base64url-no-pad encoded exactly like `new_id()` — so
/// a generated id is indistinguishable from any other opaque id in this
/// codebase (in particular, still free of the `=` padding Meilisearch
/// rejects in a document id). Not cryptographic and not meant to be: the
/// only property this relies on is that the same input always maps to the
/// same output, which FNV-1a (unlike `HashMap`'s randomized default hasher)
/// guarantees without pulling in a new crate.
pub fn deterministic_id(seed: &str) -> String {
    let bytes = seed.as_bytes();
    let hi = fnv1a_64(bytes, 0xcbf2_9ce4_8422_2325);
    let lo = fnv1a_64(bytes, 0x9e37_79b9_7f4a_7c15);
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&hi.to_be_bytes());
    buf[8..].copy_from_slice(&lo.to_be_bytes());
    BASE64_URL_SAFE_NO_PAD.encode(buf)
}

/// FNV-1a-64 (see e.g. http://www.isthe.com/chongo/tech/comp/fnv/): a tiny,
/// deterministic, non-cryptographic hash — exactly what [`deterministic_id`]
/// needs and nothing more.
fn fnv1a_64(bytes: &[u8], offset_basis: u64) -> u64 {
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = offset_basis;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

impl Dao {
    /// Create a series meta item. `Conflict` if the series id already exists.
    #[tracing::instrument(skip(self, series), fields(series_id = %series.id))]
    pub async fn create_series(&self, series: &SeriesRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Series(series.id.clone()),
            &Sk::Meta,
            TYPE_SERIES,
            series,
        )?;

        let result = self
            .client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(format!(
                "series {} already exists",
                series.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a series by id. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_series(&self, series_id: &str) -> DaoResult<Option<SeriesRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Bump `occurrences_generated` from `expected` to `expected + 1`,
    /// guarded on it still reading `expected` — the same "expected old
    /// value" idempotency guard as e.g. `Dao::put_rating`'s
    /// `rating_update`. `materialize_series` calls this once per occurrence
    /// it generates, with `expected` = that occurrence's 0-based index (the
    /// count already generated before it). A retry of the same activity
    /// (redelivery, or a resumed workflow replaying past an occurrence it
    /// already created) calls this again with the same `expected`; by then
    /// the stored value has moved on, so the condition fails — treated as
    /// success, not a double count, since the bump this call wanted has
    /// already happened.
    #[tracing::instrument(skip(self))]
    pub async fn bump_series_occurrences_generated(
        &self,
        series_id: &str,
        expected: u32,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET occurrences_generated = :new")
            .condition_expression("attribute_exists(#pk) AND occurrences_generated = :expected")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":new", AttributeValue::N((expected + 1).to_string()))
            .expression_attribute_values(":expected", AttributeValue::N(expected.to_string()))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Move a series from status `from` to `to`, guarded on it still reading
    /// `from` — same idempotency role as
    /// [`Self::bump_series_occurrences_generated`]. `materialize_series`
    /// uses this once, at the end of a successful run, to flip
    /// `"generating"` → `"active"`; a replay/retry of that final step finds
    /// the status already moved on and is a no-op success rather than an
    /// error.
    #[tracing::instrument(skip(self))]
    pub async fn update_series_status(
        &self,
        series_id: &str,
        from: &str,
        to: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Series(series_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET #status = :to")
            .condition_expression("attribute_exists(#pk) AND #status = :from")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":to", s(to))
            .expression_attribute_values(":from", s(from))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => Ok(()),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(
                se.err(),
                PutItemError::ConditionalCheckFailedException(_)
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `series_occurrence_id`: the same `(series_id,
    /// index)` always yields the same id, so a retried `materialize_series`
    /// activity lands on the id it (or an earlier attempt) already used.
    #[test]
    fn series_occurrence_id_is_deterministic() {
        assert_eq!(
            series_occurrence_id("sr1", 3),
            series_occurrence_id("sr1", 3)
        );
    }

    /// Different occurrences of the same series, and the same occurrence
    /// index of different series, must land on different ids — otherwise
    /// `materialize_series` would collide two genuinely distinct
    /// occurrences onto one match id.
    #[test]
    fn series_occurrence_id_differs_across_index_and_series() {
        let ids: Vec<String> = (0..20)
            .map(|i| series_occurrence_id("sr1", i))
            .chain((0..20).map(|i| series_occurrence_id("sr2", i)))
            .collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "collision among {ids:?}");
    }

    /// The id must be usable verbatim as a Meilisearch document id (and as
    /// any other match id is) — no `=` padding, only the base64url alphabet.
    #[test]
    fn series_occurrence_id_has_no_padding() {
        let id = series_occurrence_id("sr1", 0);
        assert!(!id.contains('='), "{id} should have no padding");
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{id} should be base64url"
        );
    }
}
