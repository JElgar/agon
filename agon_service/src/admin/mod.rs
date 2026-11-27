//! Admin panel: its own `OpenApi`-bearing type (`AdminApi`), combined with
//! the rest of the service's `Api` via `OpenApiService::new((Api,
//! AdminApi), ...)` in `main.rs` — the first endpoint group pulled out of
//! that file's one big `impl Api` block, as a first step toward splitting
//! more of it into its own modules.
//!
//! Types live in [`models`] — see that module's doc comment for why they're
//! referenced qualified (`models::User`, not a bare `User`).

mod backfill;
pub mod models;

use agon_services_core::dao;
use poem::http::StatusCode;
use poem::web::Data;
use poem::{Error, Result};
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, OpenApi};

use crate::auth::JwtClaims;
use crate::mapping::{dao_internal, parse_ts};
use crate::{opensearch_internal, page_limit, require_uid, search_cursor, search_offset};

pub use backfill::BackfillClient;

/// Map a backfill run (or its absence, before one has ever been triggered)
/// to the API's representation.
fn backfill_status_from_run(run: Option<backfill::BackfillRun>) -> models::BackfillStatus {
    let Some(run) = run else {
        return models::BackfillStatus {
            status: "idle".into(),
            delete_old: None,
            started_at: None,
            finished_at: None,
            match_pages: 0,
            matches_indexed: 0,
            matches_removed: 0,
            matches_failed: 0,
            user_pages: 0,
            users_indexed: 0,
            users_removed: 0,
            users_failed: 0,
            error: None,
        };
    };
    let error = (run.status == backfill::RunStatus::Failed)
        .then(|| "backfill failed; see agon-worker logs for the cause".to_string());
    models::BackfillStatus {
        status: match run.status {
            backfill::RunStatus::Running => "running",
            backfill::RunStatus::Succeeded => "succeeded",
            backfill::RunStatus::Failed => "failed",
        }
        .to_string(),
        delete_old: Some(run.progress.delete_old),
        started_at: run.started_at,
        finished_at: run.finished_at,
        match_pages: run.progress.match_pages,
        matches_indexed: run.progress.matches_indexed,
        matches_removed: run.progress.matches_removed,
        matches_failed: run.progress.matches_failed,
        user_pages: run.progress.user_pages,
        users_indexed: run.progress.users_indexed,
        users_removed: run.progress.users_removed,
        users_failed: run.progress.users_failed,
        error,
    }
}

pub struct AdminApi;

/// Like `require_uid`, but also requires the caller's account to have
/// `is_admin` set. Called once per request by `admin_auth_middleware`
/// (`main.rs`, wrapping every `/admin/...` route) rather than by each
/// handler below — a real but non-admin account gets a 403, the same signal
/// a stranger would get probing an endpoint they have no business calling.
pub(crate) async fn require_admin(dao: &dao::Dao, jwt: &JwtClaims) -> Result<String> {
    let uid = require_uid(dao, jwt).await?;
    let user = dao
        .get_user(&uid)
        .await
        .map_err(dao_internal)?
        .ok_or_else(|| Error::from_string("user not found", StatusCode::UNAUTHORIZED))?;
    if !user.is_admin {
        return Err(Error::from_string(
            "admin access required",
            StatusCode::FORBIDDEN,
        ));
    }
    Ok(uid)
}

/// Map one `matches` index row to its admin-panel representation.
fn admin_match_from_row(row: agon_services_core::opensearch::matches::MatchRow) -> models::Match {
    models::Match {
        id: row.match_id,
        name: row.name,
        sport: row.sport,
        status: row.status,
        starts_at: parse_ts(&row.starts_at),
    }
}

/// Map a full `UserRecord` to its admin-panel representation.
fn admin_user_from_record(record: &dao::records::UserRecord) -> models::User {
    models::User {
        id: record.id.clone(),
        email: record.email.clone(),
        name: record.name.clone(),
        is_admin: record.is_admin,
        follower_count: record.follower_count as u32,
        following_count: record.following_count as u32,
        created_at: parse_ts(&record.created_at),
    }
}

#[derive(ApiResponse)]
enum GetStatsResponse {
    #[oai(status = 200)]
    Stats(Json<models::Stats>),

    /// `OPENSEARCH_URL` isn't set on this deployment.
    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListUsersResponse {
    #[oai(status = 200)]
    Users(Json<models::UserPage>),

    /// The `cursor` query parameter isn't a valid offset.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// `OPENSEARCH_URL` isn't set on this deployment.
    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum ListMatchesResponse {
    #[oai(status = 200)]
    Matches(Json<models::MatchPage>),

    /// The `cursor` query parameter isn't a valid offset.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),

    /// `OPENSEARCH_URL` isn't set on this deployment.
    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetUserResponse {
    #[oai(status = 200)]
    User(Json<models::User>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum UpdateUserResponse {
    #[oai(status = 200)]
    User(Json<models::User>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum StartBackfillResponse {
    #[oai(status = 202)]
    Started(Json<models::BackfillStatus>),

    /// A backfill triggered by an earlier request is still running.
    #[oai(status = 409)]
    AlreadyRunning(PlainText<String>),

    /// `OPENSEARCH_URL` isn't set on this deployment.
    #[oai(status = 503)]
    NotConfigured(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetBackfillStatusResponse {
    #[oai(status = 200)]
    Status(Json<models::BackfillStatus>),
}

#[OpenApi]
impl AdminApi {
    /// Admin panel: usage aggregates shown at the top of each resource page.
    /// Reads the `users`/`matches` OpenSearch indices, not Meilisearch: an
    /// exact document count via OpenSearch's own `_count` endpoint, not
    /// coupled to Meilisearch's stats response shape (which varies enough
    /// across versions to break a naive count). `GET /admin/users` and `GET
    /// /admin/matches` below read the same indices too; the user-facing
    /// search endpoints are separate and still on Meilisearch.
    #[oai(path = "/admin/stats", method = "get")]
    async fn get_stats(
        &self,
        Data(stats): Data<&crate::team_stats::StatsStore>,
    ) -> Result<GetStatsResponse> {
        let Some(os) = &stats.0 else {
            return Ok(GetStatsResponse::NotConfigured(PlainText(
                "stats are not configured on this deployment (OPENSEARCH_URL is unset)".into(),
            )));
        };
        let total_users = os
            .count(&agon_services_core::opensearch::USERS.alias(os.prefix()))
            .await
            .map_err(opensearch_internal)?;
        let total_matches = os
            .count(&agon_services_core::opensearch::MATCHES.alias(os.prefix()))
            .await
            .map_err(opensearch_internal)?;
        Ok(GetStatsResponse::Stats(Json(models::Stats {
            total_users,
            total_matches,
        })))
    }

    /// Admin panel: list/search every match (name, sport, status, kick-off).
    /// Reads the `matches` OpenSearch index
    /// (`agon_services_core::opensearch::matches`) — the same index the
    /// team-stats reads use, so no new worker sync is needed. An empty `q`
    /// browses everyone, newest first; `sport`/`status` are exact filters
    /// and `from`/`to` bound `starts_at`. Links out to the match's own page
    /// rather than offering a separate admin detail view.
    #[oai(path = "/admin/matches", method = "get")]
    async fn list_matches(
        &self,
        Data(stats): Data<&crate::team_stats::StatsStore>,
        /// Free-text search over the match name. Omit or empty to list everyone.
        #[oai(name = "q")]
        Query(query): Query<Option<String>>,
        Query(sport): Query<Option<crate::MatchType>>,
        Query(status): Query<Option<crate::MatchStatus>>,
        /// Only matches starting at or after this time.
        Query(from): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Only matches starting at or before this time.
        Query(to): Query<Option<chrono::DateTime<chrono::Utc>>>,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListMatchesResponse> {
        let Some(os) = &stats.0 else {
            return Ok(ListMatchesResponse::NotConfigured(PlainText(
                "stats are not configured on this deployment (OPENSEARCH_URL is unset)".into(),
            )));
        };
        let offset = match search_offset(cursor.as_deref()) {
            Ok(o) => o,
            Err(()) => {
                return Ok(ListMatchesResponse::ValidationError(PlainText(
                    "invalid cursor".into(),
                )));
            }
        };
        let limit = page_limit(limit);
        let sports: Vec<&str> = sport.iter().map(crate::mapping::match_type_tag).collect();
        let status = status.as_ref().map(crate::mapping::match_status_str);
        let from = from.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let to = to.map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        let filters = agon_services_core::opensearch::matches::MatchFilters {
            q: query.as_deref(),
            sports: &sports,
            status,
            from: from.as_deref(),
            to: to.as_deref(),
            ..Default::default()
        };
        let (hits, total) =
            agon_services_core::opensearch::matches::search_matches(os, filters, offset, limit)
                .await
                .map_err(opensearch_internal)?;
        let rows: Vec<_> = hits.into_iter().map(|h| h.row).collect();

        let consumed = offset.saturating_add(rows.len() as u32);
        let next_offset = ((consumed as u64) < total).then_some(consumed);
        Ok(ListMatchesResponse::Matches(Json(models::MatchPage {
            items: rows.into_iter().map(admin_match_from_row).collect(),
            next_cursor: search_cursor(next_offset),
        })))
    }

    /// Admin panel: list/search every user, full account detail. Reads the
    /// `users` OpenSearch index (`agon_services_core::opensearch::users`),
    /// not Meilisearch — an empty `q` browses everyone, newest first.
    /// Series/teams get their own `/admin/...` list endpoints as those
    /// resources are added to the panel.
    #[oai(path = "/admin/users", method = "get")]
    async fn list_users(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(stats): Data<&crate::team_stats::StatsStore>,
        /// Free-text search over name/email. Omit or empty to list everyone.
        #[oai(name = "q")]
        Query(query): Query<Option<String>>,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListUsersResponse> {
        let Some(os) = &stats.0 else {
            return Ok(ListUsersResponse::NotConfigured(PlainText(
                "stats are not configured on this deployment (OPENSEARCH_URL is unset)".into(),
            )));
        };
        let offset = match search_offset(cursor.as_deref()) {
            Ok(o) => o,
            Err(()) => {
                return Ok(ListUsersResponse::ValidationError(PlainText(
                    "invalid cursor".into(),
                )));
            }
        };
        let limit = page_limit(limit);
        let (ids, total) =
            agon_services_core::opensearch::search_users(os, query.as_deref(), offset, limit)
                .await
                .map_err(opensearch_internal)?;

        let records = dao.batch_get_users(&ids).await.map_err(dao_internal)?;
        let items = ids
            .iter()
            .filter_map(|id| records.get(id))
            .map(admin_user_from_record)
            .collect();

        let consumed = offset.saturating_add(ids.len() as u32);
        let next_offset = ((consumed as u64) < total).then_some(consumed);
        Ok(ListUsersResponse::Users(Json(models::UserPage {
            items,
            next_cursor: search_cursor(next_offset),
        })))
    }

    /// Admin panel: fetch one user's full account detail by id.
    #[oai(path = "/admin/users/:user_id", method = "get")]
    async fn get_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        Path(user_id): Path<String>,
    ) -> Result<GetUserResponse> {
        match dao.get_user(&user_id).await.map_err(dao_internal)? {
            Some(record) => Ok(GetUserResponse::User(Json(admin_user_from_record(&record)))),
            None => Ok(GetUserResponse::NotFound(PlainText(
                "user not found".into(),
            ))),
        }
    }

    /// Admin panel: edit a user's name and/or grant/revoke admin access.
    /// Deletion isn't supported yet — a user has too much referencing data
    /// (matches, teams, follows) to remove safely without a dedicated
    /// cleanup pass, tracked as follow-up work rather than done half-right
    /// here.
    #[oai(path = "/admin/users/:user_id", method = "patch")]
    async fn update_user(
        &self,
        Data(dao): Data<&dao::Dao>,
        Path(user_id): Path<String>,
        input: Json<models::UpdateUserInput>,
    ) -> Result<UpdateUserResponse> {
        let input = input.0;

        if let Some(name) = input.name.as_deref() {
            match dao.update_user_profile(&user_id, Some(name), None).await {
                Ok(()) => {}
                Err(dao::DaoError::NotFound(_)) => {
                    return Ok(UpdateUserResponse::NotFound(PlainText(
                        "user not found".into(),
                    )));
                }
                Err(e) => return Err(dao_internal(e)),
            }
        }
        if let Some(is_admin) = input.is_admin {
            match dao.set_user_admin(&user_id, is_admin).await {
                Ok(()) => {}
                Err(dao::DaoError::NotFound(_)) => {
                    return Ok(UpdateUserResponse::NotFound(PlainText(
                        "user not found".into(),
                    )));
                }
                Err(e) => return Err(dao_internal(e)),
            }
        }

        let record = dao
            .get_user(&user_id)
            .await
            .map_err(dao_internal)?
            .ok_or_else(|| Error::from_string("user not found", StatusCode::NOT_FOUND))?;
        Ok(UpdateUserResponse::User(Json(admin_user_from_record(
            &record,
        ))))
    }

    /// Admin panel: start the OpenSearch backfill as a Temporal workflow
    /// (`agon_services_core::opensearch::backfill_workflow`), instead of a
    /// manual `kubectl exec` into the worker pod. `agon_worker`'s Temporal
    /// worker runs it; this just starts (or attaches to) the fixed
    /// `opensearch-backfill` workflow id, so the run survives this process
    /// restarting. Refused with 409 while an earlier run is still going;
    /// poll `GET` on the same path for progress.
    #[oai(path = "/admin/opensearch/backfill", method = "post")]
    async fn start_opensearch_backfill(
        &self,
        Data(stats): Data<&crate::team_stats::StatsStore>,
        Data(temporal): Data<&Option<BackfillClient>>,
        input: Json<models::StartBackfillInput>,
    ) -> Result<StartBackfillResponse> {
        if stats.0.is_none() {
            return Ok(StartBackfillResponse::NotConfigured(PlainText(
                "stats are not configured on this deployment (OPENSEARCH_URL is unset)".into(),
            )));
        }
        let Some(temporal) = temporal else {
            return Ok(StartBackfillResponse::NotConfigured(PlainText(
                "Temporal is not reachable on this deployment".into(),
            )));
        };
        let delete_old = input.0.delete_old.unwrap_or(false);
        match temporal
            .start(delete_old)
            .await
            .map_err(temporal_internal)?
        {
            backfill::StartOutcome::Started => {}
            backfill::StartOutcome::AlreadyRunning => {
                return Ok(StartBackfillResponse::AlreadyRunning(PlainText(
                    "a backfill is already running".into(),
                )));
            }
        }
        let run = temporal.current_run().await.map_err(temporal_internal)?;
        Ok(StartBackfillResponse::Started(Json(
            backfill_status_from_run(run),
        )))
    }

    /// Admin panel: the OpenSearch backfill's current (or most recent) run.
    /// `status: "idle"` means a backfill has never been triggered via this
    /// endpoint.
    #[oai(path = "/admin/opensearch/backfill", method = "get")]
    async fn get_opensearch_backfill(
        &self,
        Data(temporal): Data<&Option<BackfillClient>>,
    ) -> Result<GetBackfillStatusResponse> {
        let Some(temporal) = temporal else {
            return Ok(GetBackfillStatusResponse::Status(Json(
                backfill_status_from_run(None),
            )));
        };
        let run = temporal.current_run().await.map_err(temporal_internal)?;
        Ok(GetBackfillStatusResponse::Status(Json(
            backfill_status_from_run(run),
        )))
    }
}

/// Map a Temporal client/connection failure to a 500 — these are
/// infrastructure errors (can't reach Temporal), not a 404/409 the caller
/// should act on differently.
fn temporal_internal(err: Box<dyn std::error::Error>) -> Error {
    Error::from_string(err.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
}
