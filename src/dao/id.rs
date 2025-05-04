use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub trait Table {
    fn table_name() -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SurrealId<T: Table>(pub String, std::marker::PhantomData<T>);

impl<T: Table> SurrealId<T> {
    pub fn new(id: impl Into<String>) -> Self {
        SurrealId(id.into(), std::marker::PhantomData)
    }

    pub fn id(&self) -> &str {
        &self.0
    }
}

impl<T: Table> fmt::Display for SurrealId<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", T::table_name(), self.0)
    }
}

impl<T: Table> Serialize for SurrealId<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, T: Table> Deserialize<'de> for SurrealId<T> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let expected_prefix = T::table_name();
        let mut parts = s.splitn(2, ':');
        match (parts.next(), parts.next()) {
            (Some(prefix), Some(id)) if prefix == expected_prefix => Ok(SurrealId::new(id)),
            _ => Err(serde::de::Error::custom(format!(
                "Expected '{}:<id>' format",
                expected_prefix
            ))),
        }
    }
}

