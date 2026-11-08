//! Tournament operations: create/get the tournament meta item, entries
//! (request/accept/withdraw), the entry-count counter, status transitions,
//! and bracket edges. Generating a tournament's matches from an accepted
//! entry list is not part of this module — the pure shape (which matches,
//! which fixtures, which squads) lives in `agon_core::tournament`; actually
//! writing them is `agon_service`'s `start` handler, via the ordinary
//! `Dao::create_match` every other match creation path uses, same split as
//! `dao::series_ops`/`agon_worker`'s `materialize_series`.

use aws_sdk_dynamodb::error::SdkError;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::AttributeValue;

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_PK, ATTR_SK, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{TournamentBracketEdgeRecord, TournamentEntryRecord, TournamentRecord};

pub const TYPE_TOURNAMENT: &str = "tournament";
pub const TYPE_TOURNAMENT_ENTRY: &str = "tournament_entry";
pub const TYPE_TOURNAMENT_BRACKET_EDGE: &str = "tournament_bracket_edge";

impl Dao {
    /// Create a tournament meta item. `Conflict` if the id already exists.
    #[tracing::instrument(skip(self, tournament), fields(tournament_id = %tournament.id))]
    pub async fn create_tournament(&self, tournament: &TournamentRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Tournament(tournament.id.clone()),
            &Sk::Meta,
            TYPE_TOURNAMENT,
            tournament,
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
                "tournament {} already exists",
                tournament.id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch a tournament by id. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_tournament(&self, tournament_id: &str) -> DaoResult<Option<TournamentRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// Move a tournament from status `from` to `to`, guarded on it still
    /// reading `from` — same idempotency role as
    /// `Dao::update_series_status`. A retry/replay that finds the status
    /// already moved on is a no-op success rather than an error.
    #[tracing::instrument(skip(self))]
    pub async fn set_tournament_status(
        &self,
        tournament_id: &str,
        from: &str,
        to: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
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

    /// Add an entry (a fresh request, an organiser's direct add, or an
    /// approved request's acceptance). `Conflict` if this entrant already
    /// has an entry — at most one entry per entrant, the same "doubles as
    /// the uniqueness guard" convention as `Sk::WaitlistEntry`.
    #[tracing::instrument(skip(self, entry), fields(tournament_id = %entry.tournament_id, entrant_id = %entry.entrant_id))]
    pub async fn create_tournament_entry(&self, entry: &TournamentEntryRecord) -> DaoResult<()> {
        let item = to_item(
            &Pk::Tournament(entry.tournament_id.clone()),
            &Sk::Entry(entry.entrant_id.clone()),
            TYPE_TOURNAMENT_ENTRY,
            entry,
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
                "{} already has an entry in tournament {}",
                entry.entrant_id, entry.tournament_id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch one entrant's entry. `None` if absent.
    #[tracing::instrument(skip(self))]
    pub async fn get_tournament_entry(
        &self,
        tournament_id: &str,
        entrant_id: &str,
    ) -> DaoResult<Option<TournamentEntryRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Entry(entrant_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
        }
    }

    /// List a tournament's entries, paginated.
    #[tracing::instrument(skip(self))]
    pub async fn list_tournament_entries(
        &self,
        tournament_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<TournamentEntryRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .key_condition_expression("#pk = :pk AND begins_with(SK, :sk)")
                .expression_attribute_names("#pk", ATTR_PK)
                .expression_attribute_values(
                    ":pk",
                    s(Pk::Tournament(tournament_id.into()).to_string()),
                )
                .expression_attribute_values(":sk", s(Sk::entry_prefix())),
            cursor,
            limit,
        )
        .await
    }

    /// Move an entry from status `from` to `to` (e.g. an organiser approving
    /// a `"requested"` entry into `"accepted"`), guarded on it still reading
    /// `from` — same idempotency role as `Dao::set_tournament_status`.
    #[tracing::instrument(skip(self))]
    pub async fn set_tournament_entry_status(
        &self,
        tournament_id: &str,
        entrant_id: &str,
        from: &str,
        to: &str,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Entry(entrant_id.into()).to_string()))
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

    /// Withdraw/remove an entry outright (only while the tournament is
    /// still `"open"` — enforced by the caller, not here).
    #[tracing::instrument(skip(self))]
    pub async fn delete_tournament_entry(
        &self,
        tournament_id: &str,
        entrant_id: &str,
    ) -> DaoResult<()> {
        self.client
            .delete_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Entry(entrant_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Adjust `entry_count` by `delta` (positive on add, negative on
    /// withdraw) — an atomic `ADD`, same convention as every other
    /// denormalized counter in this DAO (never recomputed from a scan).
    #[tracing::instrument(skip(self))]
    pub async fn bump_tournament_entry_count(
        &self,
        tournament_id: &str,
        delta: i64,
    ) -> DaoResult<()> {
        self.client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("ADD entry_count :delta")
            .expression_attribute_values(":delta", AttributeValue::N(delta.to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        Ok(())
    }

    /// Write a knockout bracket edge. Generation-time only (one put per
    /// non-final match, each at a distinct `match_id` it'll never collide
    /// on), so a conditional put is defensive rather than load-bearing —
    /// unlike `Dao::claim_series_occurrence`, nothing here retries a failed
    /// generation attempt onto the same tournament.
    #[tracing::instrument(skip(self, edge), fields(tournament_id = %edge.tournament_id, match_id = %edge.match_id))]
    pub async fn create_tournament_bracket_edge(
        &self,
        edge: &TournamentBracketEdgeRecord,
    ) -> DaoResult<()> {
        let item = to_item(
            &Pk::Tournament(edge.tournament_id.clone()),
            &Sk::BracketEdge(edge.match_id.clone()),
            TYPE_TOURNAMENT_BRACKET_EDGE,
            edge,
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
                "bracket edge for match {} already exists",
                edge.match_id
            ))),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Fetch one match's bracket edge. `None` for a match with no edge (the
    /// final, or any match outside a knockout tournament).
    #[tracing::instrument(skip(self))]
    pub async fn get_tournament_bracket_edge(
        &self,
        tournament_id: &str,
        match_id: &str,
    ) -> DaoResult<Option<TournamentBracketEdgeRecord>> {
        let out = self
            .client
            .get_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::BracketEdge(match_id.into()).to_string()))
            .send()
            .await
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;
        match out.item {
            Some(item) => Ok(Some(from_item(item)?)),
            None => Ok(None),
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
