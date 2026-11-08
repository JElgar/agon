//! Admin panel: its own `OpenApi`-bearing type (`AdminApi`), combined with
//! the rest of the service's `Api` via `OpenApiService::new((Api,
//! AdminApi), ...)` in `main.rs` — the first endpoint group pulled out of
//! that file's one big `impl Api` block, as a first step toward splitting
//! more of it into its own modules.
//!
//! Types live in [`models`] — see that module's doc comment for why they're
//! referenced qualified (`models::User`, not a bare `User`).

pub mod models;

use agon_core::dao;
use poem::http::StatusCode;
use poem::web::Data;
use poem::{Error, Result};
use poem_openapi::param::{Path, Query};
use poem_openapi::payload::{Json, PlainText};
use poem_openapi::{ApiResponse, OpenApi};

use crate::auth::JwtClaims;
use crate::mapping::{dao_internal, parse_ts};
use crate::{page_limit, require_uid, search_cursor, search_internal, search_offset};

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
enum ListUsersResponse {
    #[oai(status = 200)]
    Users(Json<models::UserPage>),

    /// The `cursor` query parameter isn't a valid offset.
    #[oai(status = 400)]
    ValidationError(PlainText<String>),
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

#[OpenApi]
impl AdminApi {
    /// Admin panel: list/search every user, full account detail. Reuses the
    /// same `users` Meilisearch index `GET /users/search` queries — an empty
    /// `q` browses everyone. Matches/series/teams get their own `/admin/...`
    /// list endpoints as those resources are added to the panel.
    #[oai(path = "/admin/users", method = "get")]
    async fn list_users(
        &self,
        Data(dao): Data<&dao::Dao>,
        Data(search): Data<&agon_core::search::SearchClient>,
        /// Free-text search over name/email. Omit or empty to list everyone.
        #[oai(name = "q")]
        Query(query): Query<Option<String>>,
        /// Opaque cursor from the previous page's `next_cursor`. Omit for the first page.
        Query(cursor): Query<Option<String>>,
        Query(limit): Query<Option<u32>>,
    ) -> Result<ListUsersResponse> {
        let offset = match search_offset(cursor.as_deref()) {
            Ok(o) => o,
            Err(()) => {
                return Ok(ListUsersResponse::ValidationError(PlainText(
                    "invalid cursor".into(),
                )));
            }
        };
        let q = agon_core::search::SearchQuery {
            q: query.unwrap_or_default(),
            offset,
            limit: page_limit(limit),
            ..Default::default()
        };
        let hits = search
            .search(agon_core::search::Index::Users, &q)
            .await
            .map_err(search_internal)?;

        let records = dao.batch_get_users(&hits.ids).await.map_err(dao_internal)?;
        let items = hits
            .ids
            .iter()
            .filter_map(|id| records.get(id))
            .map(admin_user_from_record)
            .collect();

        Ok(ListUsersResponse::Users(Json(models::UserPage {
            items,
            next_cursor: search_cursor(hits.next_offset),
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
}
