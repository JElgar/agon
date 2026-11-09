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
use aws_sdk_dynamodb::types::{AttributeValue, Delete, Put, TransactWriteItem, Update};

use super::client::Dao;
use super::error::{DaoError, DaoResult};
use super::is_update_conditional_failure;
use super::item::{ATTR_GSI1PK, ATTR_PK, ATTR_SK, ItemBuilder, from_item, s, to_item};
use super::keys::{Pk, Sk};
use super::page::Page;
use super::records::{
    MatchRecord, SquadLabelRecord, TournamentBracketEdgeRecord, TournamentEntryAudienceRecord,
    TournamentEntryRecord, TournamentRecord,
};

pub const TYPE_TOURNAMENT: &str = "tournament";
pub const TYPE_TOURNAMENT_ENTRY: &str = "tournament_entry";
pub const TYPE_TOURNAMENT_BRACKET_EDGE: &str = "tournament_bracket_edge";

impl Dao {
    /// Create a tournament meta item. `Conflict` if the id already exists.
    /// Projected into GSI1 (`UTOURNAMENTS#<createdByUserId>`) so the creator
    /// can list it back via `Dao::list_user_tournament_ids`, alongside any
    /// tournament they're a direct entrant in rather than the organiser of.
    #[tracing::instrument(skip(self, tournament), fields(tournament_id = %tournament.id))]
    pub async fn create_tournament(&self, tournament: &TournamentRecord) -> DaoResult<()> {
        let item = ItemBuilder::new(to_item(
            &Pk::Tournament(tournament.id.clone()),
            &Sk::Meta,
            TYPE_TOURNAMENT,
            tournament,
        )?)
        .gsi1(
            format!("UTOURNAMENTS#{}", tournament.created_by_user_id),
            format!("{}#{}", tournament.created_at, tournament.id),
        )
        .build();
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

    /// Fetch multiple tournaments in one batch read, keyed by tournament id.
    /// An id with no tournament simply has no entry in the result — mirrors
    /// `batch_get_team_metas`/`batch_get_users`. Used to hydrate
    /// `MatchTournamentInfo` for a whole feed/search page in one round trip
    /// instead of one `get_tournament` per match.
    #[tracing::instrument(skip(self))]
    pub async fn batch_get_tournaments(
        &self,
        tournament_ids: &[String],
    ) -> DaoResult<std::collections::HashMap<String, TournamentRecord>> {
        // De-dup — BatchGetItem rejects duplicate keys in one request.
        let mut seen = std::collections::HashSet::new();
        let keys: Vec<_> = tournament_ids
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .map(|id| {
                std::collections::HashMap::from([
                    (
                        ATTR_PK.to_string(),
                        s(Pk::Tournament(id.clone()).to_string()),
                    ),
                    (ATTR_SK.to_string(), s(Sk::Meta.to_string())),
                ])
            })
            .collect();

        let items = self.batch_get_all(keys, None).await?;
        let mut out = std::collections::HashMap::with_capacity(items.len());
        for item in items {
            let record: TournamentRecord = from_item(item)?;
            out.insert(record.id.clone(), record);
        }
        Ok(out)
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

    /// Set (or replace) one squad's name/colour on the tournament meta item
    /// (`TournamentRecord::squad_labels`), keyed by squad index. No
    /// existence check on the squad itself — a squad is just an index an
    /// entry's `squad` field points at, so there's nothing else to validate
    /// against before the tournament starts.
    #[tracing::instrument(skip(self))]
    pub async fn set_tournament_squad_label(
        &self,
        tournament_id: &str,
        squad: u32,
        label: &SquadLabelRecord,
    ) -> DaoResult<()> {
        let label_av =
            serde_dynamo::to_attribute_value(label).map_err(|e| DaoError::Dynamo(e.to_string()))?;
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("SET squad_labels.#squad = :label")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#squad", squad.to_string())
            .expression_attribute_values(":label", label_av)
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("tournament {tournament_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Add an entry (a fresh request, an organiser's direct add, or an
    /// approved request's acceptance) and bump the tournament's
    /// `entry_count` atomically in one `TransactWriteItems`, so the counter
    /// can never drift from the actual entry rows (e.g. on a crash between
    /// the two writes). `Conflict` if this entrant already has an entry —
    /// at most one entry per entrant, the same "doubles as the uniqueness
    /// guard" convention as `Sk::WaitlistEntry`.
    /// `audience` decides the entry's GSI1 projection: on a `Players`
    /// tournament `entrant_id` is itself a user id, so it's projected into
    /// the same `UTOURNAMENTS#<userId>` namespace `create_tournament` uses
    /// for "created by me" — on a `Teams` tournament `entrant_id` is a team
    /// id instead, so it's projected into `TEAMTOURNAMENTS#<teamId>` and
    /// resolved back to the user via their team memberships (see
    /// `Dao::list_team_tournament_ids`).
    #[tracing::instrument(skip(self, entry), fields(tournament_id = %entry.tournament_id, entrant_id = %entry.entrant_id))]
    pub async fn create_tournament_entry(
        &self,
        entry: &TournamentEntryRecord,
        audience: TournamentEntryAudienceRecord,
    ) -> DaoResult<()> {
        let gsi1_pk = match audience {
            TournamentEntryAudienceRecord::Players => format!("UTOURNAMENTS#{}", entry.entrant_id),
            TournamentEntryAudienceRecord::Teams => {
                format!("TEAMTOURNAMENTS#{}", entry.entrant_id)
            }
        };
        let item = ItemBuilder::new(to_item(
            &Pk::Tournament(entry.tournament_id.clone()),
            &Sk::Entry(entry.entrant_id.clone()),
            TYPE_TOURNAMENT_ENTRY,
            entry,
        )?)
        .gsi1(
            gsi1_pk,
            format!("{}#{}", entry.created_at, entry.tournament_id),
        )
        .build();
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
                    .update(self.tournament_entry_count_counter(&entry.tournament_id, 1)?)
                    .build(),
            )
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if super::is_transaction_conditional_failure(&e) => {
                Err(DaoError::Conflict(format!(
                    "{} already has an entry in tournament {}",
                    entry.entrant_id, entry.tournament_id
                )))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    fn tournament_entry_count_counter(&self, tournament_id: &str, delta: i64) -> DaoResult<Update> {
        Update::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression("ADD entry_count :delta")
            .expression_attribute_values(":delta", AttributeValue::N(delta.to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))
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

    /// Assign (or reassign) an entrant's fixed squad, for a teamless
    /// tournament under manual `team_formation`. No idempotency guard
    /// needed — unlike a status transition, the organiser can change their
    /// mind about a squad assignment freely, any number of times, while the
    /// tournament is still `"open"` (enforced by the caller, not here).
    #[tracing::instrument(skip(self))]
    pub async fn set_tournament_entry_squad(
        &self,
        tournament_id: &str,
        entrant_id: &str,
        squad: u32,
    ) -> DaoResult<()> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Entry(entrant_id.into()).to_string()))
            .update_expression("SET squad = :squad")
            .condition_expression("attribute_exists(#pk)")
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_values(":squad", AttributeValue::N(squad.to_string()))
            .send()
            .await;
        match result {
            Ok(_) => Ok(()),
            Err(e) if is_update_conditional_failure(&e) => {
                Err(DaoError::NotFound(format!("entry {entrant_id}")))
            }
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// Withdraw/remove an entry outright (only while the tournament is
    /// still `"open"` — enforced by the caller, not here).
    #[tracing::instrument(skip(self))]
    /// Remove an entry and decrement the tournament's `entry_count`
    /// atomically in one `TransactWriteItems` — same reasoning as
    /// `create_tournament_entry`: the counter must never be able to drift
    /// from the actual entry rows.
    #[tracing::instrument(skip(self))]
    pub async fn delete_tournament_entry(
        &self,
        tournament_id: &str,
        entrant_id: &str,
    ) -> DaoResult<()> {
        let delete = Delete::builder()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Tournament(tournament_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Entry(entrant_id.into()).to_string()))
            .build()
            .map_err(|e| DaoError::Dynamo(e.to_string()))?;

        self.client
            .transact_write_items()
            .transact_items(TransactWriteItem::builder().delete(delete).build())
            .transact_items(
                TransactWriteItem::builder()
                    .update(self.tournament_entry_count_counter(tournament_id, -1)?)
                    .build(),
            )
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

    /// Fill a knockout bracket match's side with the winning team, once its
    /// feeder match's score is confirmed (see `agon_worker`'s tournament
    /// progression handler). Guarded on the side still being empty
    /// (`attribute_not_exists(sides.#sid.team_id)`) — a confirmed score
    /// never un-confirms, so a retry/replay that finds the side already
    /// filled is a harmless no-op success, same idempotency role as
    /// `Dao::set_tournament_status`. Also clears the placeholder `"TBD"`
    /// name and turns on `team_join_enabled`, same shape as an ordinary
    /// team-linked side.
    #[tracing::instrument(skip(self))]
    /// Returns `true` if this call actually performed the fill, `false` if
    /// the side was already filled (redelivery/edit no-op) — callers that
    /// need to do more work only on a genuine first fill (e.g. copying a
    /// teamless squad's roster alongside, in
    /// [`Self::fill_tournament_bracket_squad_side`]) use that to avoid
    /// double-adding.
    pub async fn fill_tournament_bracket_side(
        &self,
        match_id: &str,
        side_id: &str,
        team_id: &str,
    ) -> DaoResult<bool> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression(
                "SET sides.#sid.team_id = :team, sides.#sid.team_join_enabled = :true REMOVE sides.#sid.#name",
            )
            .condition_expression(
                "attribute_exists(#pk) AND attribute_exists(sides.#sid) AND attribute_not_exists(sides.#sid.team_id)",
            )
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#sid", side_id)
            .expression_attribute_names("#name", "name")
            .expression_attribute_values(":team", s(team_id))
            .expression_attribute_values(":true", AttributeValue::Bool(true))
            .send()
            .await;
        match result {
            Ok(_) => Ok(true),
            Err(e) if is_update_conditional_failure(&e) => Ok(false),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// The teamless-tournament counterpart to
    /// [`Self::fill_tournament_bracket_side`]: `team_id` is a synthetic
    /// squad id, not a real team, so there's no team to resolve a name/logo
    /// or self-join from — `squad_name`/`squad_colour` are written directly
    /// instead of being removed, and `team_join_enabled` is left `false`.
    /// Returns `true` on a genuine first fill, same meaning as the other
    /// method — the caller still has to copy the squad's actual player
    /// roster across separately (via `add_match_player_tx`), since that
    /// isn't part of this single-item update.
    pub async fn fill_tournament_bracket_squad_side(
        &self,
        match_id: &str,
        side_id: &str,
        squad_id: &str,
        squad_name: &str,
        squad_colour: &str,
    ) -> DaoResult<bool> {
        let result = self
            .client
            .update_item()
            .table_name(self.table())
            .key(ATTR_PK, s(Pk::Match(match_id.into()).to_string()))
            .key(ATTR_SK, s(Sk::Meta.to_string()))
            .update_expression(
                "SET sides.#sid.team_id = :team, sides.#sid.#name = :name, sides.#sid.colour = :colour",
            )
            .condition_expression(
                "attribute_exists(#pk) AND attribute_exists(sides.#sid) AND attribute_not_exists(sides.#sid.team_id)",
            )
            .expression_attribute_names("#pk", ATTR_PK)
            .expression_attribute_names("#sid", side_id)
            .expression_attribute_names("#name", "name")
            .expression_attribute_values(":team", s(squad_id))
            .expression_attribute_values(":name", s(squad_name))
            .expression_attribute_values(":colour", s(squad_colour))
            .send()
            .await;
        match result {
            Ok(_) => Ok(true),
            Err(e) if is_update_conditional_failure(&e) => Ok(false),
            Err(e) => Err(DaoError::Dynamo(e.to_string())),
        }
    }

    /// List a tournament's generated matches via their GSI1 projection
    /// (`TMATCHES#<id>` / `<round>#<matchId>` — see `Dao::create_match`),
    /// ordered by round. Paginated — used by standings computation and any
    /// future `GET /tournaments/{id}/matches`.
    #[tracing::instrument(skip(self))]
    pub async fn list_tournament_matches(
        &self,
        tournament_id: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> DaoResult<Page<MatchRecord>> {
        self.query_page(
            self.client
                .query()
                .table_name(self.table())
                .index_name("GSI1")
                .key_condition_expression("#pk = :pk")
                .expression_attribute_names("#pk", ATTR_GSI1PK)
                .expression_attribute_values(":pk", s(format!("TMATCHES#{tournament_id}"))),
            cursor,
            limit,
        )
        .await
    }

    /// Tournament ids this user is directly tied to — created by them, or
    /// (on a `Players`-audience tournament) entered by them directly — via
    /// GSI1 (`UTOURNAMENTS#<userId>`), newest first. Walks every page itself
    /// rather than returning one, since the caller (`GET /tournaments/mine`)
    /// merges this with `list_team_tournament_ids` before paging the result.
    /// Does not include a tournament entered only by a team this user
    /// belongs to — see `list_team_tournament_ids` for that.
    #[tracing::instrument(skip(self))]
    pub async fn list_user_tournament_ids(&self, user_id: &str) -> DaoResult<Vec<String>> {
        self.list_tournament_ref_ids(format!("UTOURNAMENTS#{user_id}"))
            .await
    }

    /// Tournament ids a team has an entry in, via GSI1
    /// (`TEAMTOURNAMENTS#<teamId>`), newest first — the `Teams`-audience
    /// counterpart to `list_user_tournament_ids`.
    #[tracing::instrument(skip(self))]
    pub async fn list_team_tournament_ids(&self, team_id: &str) -> DaoResult<Vec<String>> {
        self.list_tournament_ref_ids(format!("TEAMTOURNAMENTS#{team_id}"))
            .await
    }

    /// Walk every page of a GSI1 partition that holds a mix of tournament
    /// meta items (`id`) and tournament entry items (`tournament_id`),
    /// resolving each to its tournament id via `TournamentRef`.
    async fn list_tournament_ref_ids(&self, gsi1_pk: String) -> DaoResult<Vec<String>> {
        let mut ids = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page: Page<TournamentRef> = self
                .query_page(
                    self.client
                        .query()
                        .table_name(self.table())
                        .index_name("GSI1")
                        .key_condition_expression("#pk = :pk")
                        .expression_attribute_names("#pk", ATTR_GSI1PK)
                        .expression_attribute_values(":pk", s(gsi1_pk.clone()))
                        .scan_index_forward(false),
                    cursor.as_deref(),
                    50,
                )
                .await?;
            ids.extend(page.items.into_iter().filter_map(TournamentRef::into_id));
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        Ok(ids)
    }
}

/// Minimal shape read off a GSI1 row that's either a tournament meta item
/// (`id`) or a tournament entry item (`tournament_id`) — just enough for
/// `Dao::list_tournament_ref_ids` to resolve back to a tournament id,
/// without needing to know which of the two it actually is.
#[derive(serde::Deserialize)]
struct TournamentRef {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    tournament_id: Option<String>,
}

impl TournamentRef {
    fn into_id(self) -> Option<String> {
        self.id.or(self.tournament_id)
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
