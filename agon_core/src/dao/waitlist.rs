//! Match waitlist: someone queued for a match/side that had no room when
//! they tried to join or accept an invite onto it, kept entirely apart from
//! the roster (`MatchPlayerRecord`) until a match admin moves them in — see
//! `MatchWaitlistEntryRecord`'s doc comment for why, and
//! `Dao::refresh_side_roster_previews` for how a waitlisted-but-accepted
//! invitee still counts toward no cap.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::{Put, TransactWriteItem};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_PK, ATTR_SK, Item, s, to_item};
use super::keys::{Pk, Sk};
use super::records::MatchWaitlistEntryRecord;

pub const TYPE_MATCH_WAITLIST_ENTRY: &str = "match_waitlist_entry";

impl Dao {
    /// List a match's waitlist, longest-waiting first. Purely informational
    /// ordering — moving someone in is always a manual admin pick, never
    /// automatic.
    #[tracing::instrument(skip(self))]
    pub async fn list_waitlist(&self, match_id: &str) -> DaoResult<Vec<MatchWaitlistEntryRecord>> {
        let mut entries = self
            .query_match_collection::<MatchWaitlistEntryRecord>(match_id, &Sk::waitlist_prefix())
            .await?;
        entries.sort_by(|a, b| a.waitlisted_at.cmp(&b.waitlisted_at));
        Ok(entries)
    }

    /// Fetch one user's waitlist entry for a match. `None` if they're not
    /// waiting.
    #[tracing::instrument(skip(self))]
    pub async fn get_waitlist_entry(
        &self,
        match_id: &str,
        user_id: &str,
    ) -> DaoResult<Option<MatchWaitlistEntryRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::WaitlistEntry(user_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(super::item::from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Join the waitlist for a match/side — a plain conditional put guarded
    /// on the caller not already waiting (`Sk::WaitlistEntry` is keyed by
    /// user id precisely so this is a single-item uniqueness guard, the same
    /// pattern as every other guard in this table). Deliberately uncapped:
    /// unlike the roster, there's no limit on how many people can queue.
    ///
    /// `Conflict` if the caller is already on this match's waitlist.
    #[tracing::instrument(skip(self, entry), fields(user_id = %entry.user_id))]
    pub async fn waitlist_join_tx(
        &self,
        match_id: &str,
        entry: &MatchWaitlistEntryRecord,
    ) -> DaoResult<()> {
        let item = self.waitlist_entry_item(match_id, entry)?;
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
            Err(e) if is_put_conditional_failure(&e) => Err(DaoError::Conflict(
                "already on this match's waitlist".into(),
            )),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Accept a pending match invitation and join the waitlist in one
    /// transaction: the invitation and its roster entry flip to accepted
    /// (exactly like `Dao::accept_invitation_tx` — so the invitee isn't asked
    /// to respond again) with the waitlist entry created alongside, atomic
    /// with both. Deleting the waitlist entry later (`Dao::leave_waitlist`,
    /// or as part of moving them in) is all that's then needed to bring them
    /// back into the match's counts — see
    /// `MatchPlayerRecord::occupies_slot`'s doc comment.
    ///
    /// `NotFound` if the invitation is gone; `Conflict` if it's not to a
    /// match, or the caller is already on this match's waitlist.
    #[tracing::instrument(skip(self, entry), fields(user_id = %entry.user_id))]
    pub async fn accept_invitation_onto_waitlist_tx(
        &self,
        invitation_id: &str,
        accepting_user_id: &str,
        responded_at: &str,
        now: &str,
        entry: &MatchWaitlistEntryRecord,
    ) -> DaoResult<String> {
        let (match_id, mut items) = self
            .accept_invitation_items(invitation_id, accepting_user_id, responded_at, now)
            .await?;
        let Some(match_id) = match_id else {
            return Err(DaoError::Conflict(format!(
                "invitation {invitation_id} is not to a match"
            )));
        };
        let waitlist_item = self.waitlist_entry_item(&match_id, entry)?;
        let put_waitlist = Put::builder()
            .table_name(self.table())
            .set_item(Some(waitlist_item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        items.push(TransactWriteItem::builder().put(put_waitlist).build());

        self.send_transact_items(items).await?;
        Ok(match_id)
    }

    /// Cancel a waitlist entry — the waiting player leaving voluntarily, a
    /// match admin clearing it, or the last step of moving someone in
    /// (`agon_service` deletes the entry once it's put/confirmed the roster
    /// row). Idempotent: deleting a missing entry is a no-op, the same
    /// convention as `Dao::remove_match_players`.
    #[tracing::instrument(skip(self))]
    pub async fn leave_waitlist(&self, match_id: &str, user_id: &str) -> DaoResult<()> {
        self.client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::WaitlistEntry(user_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    fn waitlist_entry_item(
        &self,
        match_id: &str,
        entry: &MatchWaitlistEntryRecord,
    ) -> DaoResult<Item> {
        to_item(
            &Pk::Match(match_id.into()),
            &Sk::WaitlistEntry(entry.user_id.clone()),
            TYPE_MATCH_WAITLIST_ENTRY,
            entry,
        )
    }
}

fn is_put_conditional_failure(err: &SdkError<PutItemError>) -> bool {
    matches!(
        err,
        SdkError::ServiceError(se)
            if matches!(se.err(), PutItemError::ConditionalCheckFailedException(_))
    )
}
