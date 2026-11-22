//! A user's connected YouTube channel: connect (upsert), read, disconnect.
//!
//! `YoutubeConnectionRecord` lives at `USER#<uid>` / `#YOUTUBE` — a
//! singleton per user, like `Sk::Profile`. Unlike `dao::paired_device`
//! (where revoking the wrong thing is worth surfacing as a 404), connecting
//! and disconnecting YouTube are both plain idempotent upserts/deletes: the
//! effect a caller wants either way is "end up connected/disconnected",
//! which point-reconnecting or point-disconnecting already achieves without
//! needing to distinguish "was already in that state" as an error.
//!
//! This module never touches the OAuth refresh token's plaintext or the key
//! that encrypts it — `encrypted_refresh_token` is opaque storage as far as
//! this crate is concerned (see `YoutubeConnectionRecord`'s doc comment).

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::YoutubeConnectionRecord;

pub const TYPE_YOUTUBE_CONNECTION: &str = "youtube_connection";

impl Dao {
    /// Create or replace the caller's YouTube connection. Reconnecting
    /// (e.g. after revoking access on Google's side, or switching accounts)
    /// simply overwrites the previous record in place.
    pub async fn upsert_youtube_connection(
        &self,
        record: &YoutubeConnectionRecord,
    ) -> DaoResult<()> {
        let item = to_item(
            &Pk::User(record.user_id.clone()),
            &Sk::YoutubeConnection,
            TYPE_YOUTUBE_CONNECTION,
            record,
        )?;
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Fetch a user's YouTube connection, if they have one.
    pub async fn get_youtube_connection(
        &self,
        user_id: &str,
    ) -> DaoResult<Option<YoutubeConnectionRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::YoutubeConnection.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Disconnect YouTube. A no-op (not an error) if the user wasn't
    /// connected — see this module's doc comment on why that's the right
    /// idempotency for this pair of operations.
    pub async fn delete_youtube_connection(&self, user_id: &str) -> DaoResult<()> {
        self.client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::User(user_id.into()).to_string()))
            .key("SK", s(Sk::YoutubeConnection.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }
}
