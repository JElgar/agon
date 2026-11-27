//! The free-text name query shared by the match and user searches.

use serde_json::{Value, json};

/// A typo-tolerant "search as you type" query over `fields`.
///
/// `bool_prefix` matches every word but the last exactly and the last as a
/// prefix, and `fuzziness: AUTO` makes the exact words typo tolerant. The last
/// word is never fuzzy there, so a plain fuzzy `multi_match` is OR'd in to
/// catch a typo in a fully typed last word. A typo in a word that is still
/// being typed isn't covered.
pub fn typo_tolerant_prefix(q: &str, fields: &[&str]) -> Value {
    json!({
        "bool": {
            "should": [
                { "multi_match": {
                    "query": q, "type": "bool_prefix", "fields": fields, "fuzziness": "AUTO"
                } },
                { "multi_match": { "query": q, "fields": fields, "fuzziness": "AUTO" } }
            ],
            "minimum_should_match": 1
        }
    })
}
