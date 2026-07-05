//! Bridging between DAO record structs and DynamoDB item maps.
//!
//! Record structs hold only their data fields (clean, no key noise). This layer
//! stamps the `PK` / `SK` / `type` attributes (and any GSI keys) onto the
//! serialized map on write, and ignores them on read (records don't declare
//! them, and serde ignores unknown attributes). The stored `id`s etc. live both
//! in the key *and* as plain attributes — normal for single-table designs.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::error::{DaoError, DaoResult};
use super::keys::{Pk, Sk};

/// Reserved attribute names for the base-table keys and the type discriminator.
pub const ATTR_PK: &str = "PK";
pub const ATTR_SK: &str = "SK";
pub const ATTR_TYPE: &str = "type";
// GSI key attribute names (overloaded; populated only by items that project).
pub const ATTR_GSI1PK: &str = "GSI1PK";
pub const ATTR_GSI1SK: &str = "GSI1SK";
pub const ATTR_GSI2PK: &str = "GSI2PK";
pub const ATTR_GSI2SK: &str = "GSI2SK";
pub const ATTR_GSI3PK: &str = "GSI3PK";
pub const ATTR_GSI3SK: &str = "GSI3SK";

/// A single DynamoDB item as an attribute map.
pub type Item = HashMap<String, AttributeValue>;

/// Serialize a data struct into an item map and stamp its base-table keys +
/// type. Extra key/index attributes can be layered on with [`ItemBuilder`].
pub fn to_item<T: Serialize>(pk: &Pk, sk: &Sk, type_tag: &str, data: &T) -> DaoResult<Item> {
    let mut item: Item = serde_dynamo::to_item(data)?;
    item.insert(ATTR_PK.into(), s(pk.to_string()));
    item.insert(ATTR_SK.into(), s(sk.to_string()));
    item.insert(ATTR_TYPE.into(), s(type_tag));
    Ok(item)
}

/// Deserialize a stored item map back into a data struct. The `PK`/`SK`/`type`
/// and GSI attributes are ignored (the struct doesn't declare them).
pub fn from_item<T: DeserializeOwned>(item: Item) -> DaoResult<T> {
    Ok(serde_dynamo::from_item(item)?)
}

/// Fluent helper to add GSI projection keys onto an item after [`to_item`].
pub struct ItemBuilder {
    item: Item,
}

impl ItemBuilder {
    pub fn new(item: Item) -> Self {
        Self { item }
    }

    /// Project this item into GSI1 with the given partition + sort key.
    pub fn gsi1(mut self, pk: impl Into<String>, sk: impl Into<String>) -> Self {
        self.item.insert(ATTR_GSI1PK.into(), s(pk.into()));
        self.item.insert(ATTR_GSI1SK.into(), s(sk.into()));
        self
    }

    pub fn gsi2(mut self, pk: impl Into<String>, sk: impl Into<String>) -> Self {
        self.item.insert(ATTR_GSI2PK.into(), s(pk.into()));
        self.item.insert(ATTR_GSI2SK.into(), s(sk.into()));
        self
    }

    pub fn gsi3(mut self, pk: impl Into<String>, sk: impl Into<String>) -> Self {
        self.item.insert(ATTR_GSI3PK.into(), s(pk.into()));
        self.item.insert(ATTR_GSI3SK.into(), s(sk.into()));
        self
    }

    pub fn build(self) -> Item {
        self.item
    }
}

/// Read a required string attribute from a stored item.
pub fn require_string(item: &Item, attr: &str) -> DaoResult<String> {
    match item.get(attr) {
        Some(AttributeValue::S(v)) => Ok(v.clone()),
        Some(_) => Err(DaoError::Malformed(format!(
            "attribute `{attr}` is not a string"
        ))),
        None => Err(DaoError::Malformed(format!("missing attribute `{attr}`"))),
    }
}

/// Parse the `PK` attribute of a stored item into a typed [`Pk`].
pub fn item_pk(item: &Item) -> DaoResult<Pk> {
    Ok(require_string(item, ATTR_PK)?.parse()?)
}

/// Parse the `SK` attribute of a stored item into a typed [`Sk`].
pub fn item_sk(item: &Item) -> DaoResult<Sk> {
    Ok(require_string(item, ATTR_SK)?.parse()?)
}

/// Convenience: a string AttributeValue.
pub fn s(v: impl Into<String>) -> AttributeValue {
    AttributeValue::S(v.into())
}
