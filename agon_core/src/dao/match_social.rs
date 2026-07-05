//! Match likes, comments (+ replies, tombstone) and score submissions — the
//! sub-collections under a match, with atomic counter maintenance.

use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::item::{ATTR_GSI1PK, ATTR_PK, ItemBuilder, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{CommentRecord, MatchLikeRecord, ScoreSubmissionRecord};

pub const TYPE_MATCH_LIKE: &str = "match_like";
pub const TYPE_COMMENT: &str = "comment";
pub const TYPE_REPLY: &str = "reply";
pub const TYPE_SCORE_SUBMISSION: &str = "score_submission";

impl Dao {
    // ---- Likes ------------------------------------------------------------

    /// Like a match. Idempotent; bumps `like_count` on the match meta only when
    /// the like edge is newly created.
    pub async fn like_match(&self, match_id: &str, user_id: &str, now: &str) -> DaoResult<()> {
        let like = MatchLikeRecord {
            match_id: match_id.into(),
            user_id: user_id.into(),
            created_at: now.into(),
        };
        let item = to_item(
            &Pk::Match(match_id.into()),
            &Sk::Like(user_id.into()),
            TYPE_MATCH_LIKE,
            &like,
        )?;

        let put = Put::builder()
            .table_name(self.table())
            .set_item(Some(item))
            .condition_expression("attribute_not_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.match_counter(match_id, "like_count", 1)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()), // already liked
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Unlike a match. Idempotent; only decrements when a like actually existed.
    pub async fn unlike_match(&self, match_id: &str, user_id: &str) -> DaoResult<()> {
        let delete = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Like(user_id.into()).to_string()))
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        let result = self
            .client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.match_counter(match_id, "like_count", -1)?)
                    .build(),
            )
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => Ok(()), // wasn't liked
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Whether the given user has liked the match (drives `i_liked`).
    pub async fn has_liked_match(&self, match_id: &str, user_id: &str) -> DaoResult<bool> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Like(user_id.into()).to_string()))
            .projection_expression(ATTR_PK)
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(out.item.is_some())
    }

    /// List users who liked a match, cursor-paginated.
    pub async fn list_match_likes(
        &self,
        match_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<MatchLikeRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(":pk", s(Pk::Match(match_id.into()).to_string()))
                .expression_attribute_values(":sk", s(Sk::Like(String::new()).prefix())),
            cursor,
            limit,
        )
        .await
    }

    // ---- Comments & replies ----------------------------------------------
    //
    // Comments/replies are addressed by id (base SK `COMMENT#<id>` /
    // `REPLY#<id>`) so mutations from id-only API paths work directly. Time
    // ordering for the list endpoints is provided by a GSI1 projection keyed
    // `MCOMMENTS#<matchId>` / `CREPLIES#<parentId>` with sort `<createdAt>#<id>`.

    /// Create a top-level comment on a match. Bumps the match `comment_count`.
    pub async fn create_comment(&self, comment: &CommentRecord) -> DaoResult<()> {
        let item = ItemBuilder::new(to_item(
            &Pk::Match(comment.match_id.clone()),
            &Sk::Comment(comment.comment_id.clone()),
            TYPE_COMMENT,
            comment,
        )?)
        .gsi1(
            format!("MCOMMENTS#{}", comment.match_id),
            format!("{}#{}", comment.created_at, comment.comment_id),
        )
        .build();
        let put = Put::builder()
            .table_name(self.table())
            .set_item(Some(item))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.match_counter(&comment.match_id, "comment_count", 1)?)
                    .build(),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Create a reply to a top-level comment, updating the parent comment's
    /// `reply_count` and the match's `comment_count`. The parent is addressed by
    /// id (`COMMENT#<parentId>`) — no timestamp needed.
    pub async fn create_reply(&self, reply: &CommentRecord) -> DaoResult<()> {
        let parent_id = reply
            .parent_id
            .as_ref()
            .ok_or_else(|| DaoError::Malformed("reply missing parent_id".into()))?;

        let reply_item = ItemBuilder::new(to_item(
            &Pk::CommentReplies(parent_id.clone()),
            &Sk::Reply(reply.comment_id.clone()),
            TYPE_REPLY,
            reply,
        )?)
        .gsi1(
            format!("CREPLIES#{parent_id}"),
            format!("{}#{}", reply.created_at, reply.comment_id),
        )
        .build();
        let put_reply = Put::builder()
            .table_name(self.table())
            .set_item(Some(reply_item))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        // Bump reply_count on the parent (a top-level comment on the match),
        // addressed by id.
        let bump_parent = Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(reply.match_id.clone()).to_string()))
            .key("SK", s(Sk::Comment(parent_id.clone()).to_string()))
            .update_expression("ADD reply_count :one")
            .expression_attribute_values(":one", AttributeValue::N("1".into()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().put(put_reply).build())
            .transact_items(TransactWriteItem::builder().update(bump_parent).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.match_counter(&reply.match_id, "comment_count", 1)?)
                    .build(),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// List top-level comments on a match, newest first, via GSI1.
    pub async fn list_comments(
        &self,
        match_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<CommentRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("MCOMMENTS#{match_id}")))
                .scan_index_forward(false),
            cursor,
            limit,
        )
        .await
    }

    /// List replies to a comment, newest first, via GSI1.
    pub async fn list_replies(
        &self,
        parent_comment_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<CommentRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("CREPLIES#{parent_comment_id}")))
                .scan_index_forward(false),
            cursor,
            limit,
        )
        .await
    }

    /// Fetch a single top-level comment by id (for authorisation / reply_count
    /// checks on edit and delete). `None` if absent.
    pub async fn get_comment(
        &self,
        match_id: &str,
        comment_id: &str,
    ) -> DaoResult<Option<CommentRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Comment(comment_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(super::item::from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Edit a top-level comment's text (addressed by id). Sets `edited_at`.
    pub async fn edit_comment(
        &self,
        match_id: &str,
        comment_id: &str,
        text: &str,
        edited_at: &str,
    ) -> DaoResult<()> {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Comment(comment_id.into()).to_string()))
            .update_expression("SET #t = :t, edited_at = :e")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#t", "text")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":t", s(text))
            .expression_attribute_values(":e", s(edited_at))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Tombstone a top-level comment that has replies (addressed by id): clear
    /// author/text, set `deleted_at`, keep the row. Does not touch counts. Use
    /// `delete_comment_hard` for reply-less comments.
    pub async fn tombstone_comment(
        &self,
        match_id: &str,
        comment_id: &str,
        deleted_at: &str,
    ) -> DaoResult<()> {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Comment(comment_id.into()).to_string()))
            .update_expression("SET deleted_at = :d REMOVE #t, author_user_id")
            .expression_attribute_names("#t", "text")
            .expression_attribute_values(":d", s(deleted_at))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Hard-delete a reply-less top-level comment (addressed by id); decrements
    /// `comment_count`.
    pub async fn delete_comment_hard(&self, match_id: &str, comment_id: &str) -> DaoResult<()> {
        let delete = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Comment(comment_id.into()).to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.match_counter(match_id, "comment_count", -1)?)
                    .build(),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    // ---- Score submissions -----------------------------------------------
    //
    // Addressed by id (base SK `SCORESUB#<id>`); time ordering for the history
    // list is via GSI1 (`MSUBMISSIONS#<matchId>` / `<submittedAt>#<id>`).

    /// Fetch a single score submission by id. `None` if absent.
    pub async fn get_score_submission(
        &self,
        match_id: &str,
        submission_id: &str,
    ) -> DaoResult<Option<ScoreSubmissionRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(
                "SK",
                s(Sk::ScoreSubmission(submission_id.into()).to_string()),
            )
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(super::item::from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Append a score submission to a match's history. Also used to overwrite an
    /// existing submission (same id) after recording a response — the full item
    /// (incl. its GSI1 projection) is rewritten.
    pub async fn put_score_submission(
        &self,
        match_id: &str,
        submission: &ScoreSubmissionRecord,
    ) -> DaoResult<()> {
        let item = ItemBuilder::new(to_item(
            &Pk::Match(match_id.into()),
            &Sk::ScoreSubmission(submission.submission_id.clone()),
            TYPE_SCORE_SUBMISSION,
            submission,
        )?)
        .gsi1(
            format!("MSUBMISSIONS#{match_id}"),
            format!("{}#{}", submission.submitted_at, submission.submission_id),
        )
        .build();
        self.client
            .put_item()
            .table_name(self.table())
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// List a match's score submissions (history), newest first, via GSI1.
    pub async fn list_score_submissions(
        &self,
        match_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<ScoreSubmissionRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("MSUBMISSIONS#{match_id}")))
                .scan_index_forward(false),
            cursor,
            limit,
        )
        .await
    }

    // ---- helpers ----------------------------------------------------------

    /// An `Update` that adds `delta` to a counter on the match `#META` item.
    fn match_counter(&self, match_id: &str, counter: &str, delta: i64) -> DaoResult<Update> {
        Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key("SK", s(Sk::Meta.to_string()))
            .update_expression("ADD #c :d")
            .expression_attribute_names("#c", counter)
            .expression_attribute_values(":d", AttributeValue::N(delta.to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))
    }
}
