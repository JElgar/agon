use std::{fs::File, io::Write};

use bigdecimal::{BigDecimal, FromPrimitive, ToPrimitive};
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use dao::Dao;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use poem::{
    EndpointExt, Error, Request, Result, Route, Server, error::InternalServerError,
    http::StatusCode, listener::TcpListener, middleware::Cors, web::Data,
};
use poem_openapi::Enum;
use poem_openapi::auth::Bearer;
use poem_openapi::param::Query;
use poem_openapi::{
    ApiResponse, Object, OpenApi, OpenApiService, SecurityScheme,
    param::Path,
    payload::{Json, PlainText},
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info};

mod dao;

#[derive(Debug, Deserialize, Serialize)]
struct JwtClaims {
    sub: String,
    exp: usize,
    iss: Option<String>,
    aud: Option<String>,
    role: Option<String>,
}

#[derive(SecurityScheme)]
#[oai(
    ty = "bearer",
    key_name = "authorization",
    key_in = "header",
    checker = "jwt_checker"
)]
struct AuthSchema(JwtClaims);

async fn jwt_checker(_req: &Request, bearer: Bearer) -> Result<JwtClaims, poem::error::Error> {
    info!("Attempting to validate JWT token");
    info!(
        "Token prefix: {}",
        &bearer.token[..std::cmp::min(20, bearer.token.len())]
    );

    // Change to change the validity of the token (set to false to fail the validation)
    let secret_key = std::env::var("JWT_SECRET").expect("JWT Secret not found");
    let decoding_key = DecodingKey::from_secret(secret_key.as_bytes());

    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false;
    validation.validate_aud = false;
    validation.validate_nbf = false;

    let token_data =
        decode::<JwtClaims>(&bearer.token, &decoding_key, &validation).map_err(|err| {
            info!("JWT invalid {:?}", err);
            Error::from_string("Invalid JWT", StatusCode::UNAUTHORIZED)
        })?;

    Ok(token_data.claims)
}

struct Api;

// impl BearerAuthorization for JwtData {
//     fn from_request(req: &Request) -> Result<Self> {
//         dbg!(req.headers());
//
//         let auth_header_value = req
//             .headers()
//             .get("authorization")
//             .and_then(|value| value.to_str().ok())
//             .ok_or_else(|| {
//                 Error::from_string("Authorization header must be set", StatusCode::UNAUTHORIZED)
//             })?;
//
//         let jwt_value = auth_header_value.strip_prefix("Bearer: ").ok_or_else(|| {
//             Error::from_string(
//                 "Authorization header must have format 'Bearer: <TOKEN>'",
//                 StatusCode::UNAUTHORIZED,
//             )
//         })?;
//
//         let secret_key = std::env::var("SUPABASE_JWT_SECRET").expect("JWT Secret not found");
//         let decoding_key = DecodingKey::from_secret(secret_key.as_bytes());
//
//         let token_data =
//             decode::<JwtData>(jwt_value, &decoding_key, &Validation::new(Algorithm::HS256))
//                 .map_err(|err| {
//                     info!("JWT invalid {:?}", err);
//                     Error::from_string("Invalid JWT", StatusCode::UNAUTHORIZED)
//                 })?;
//
//         Ok(token_data.claims)
//     }
// }

#[derive(Object)]
struct User {
    id: String,
    email: String,
    first_name: String,
    last_name: String,
    username: String,
}

#[derive(Object)]
struct GroupListItem {
    id: String,
    name: String,
}

#[derive(Object)]
struct Group {
    id: String,
    name: String,
    members: Vec<User>,
}

fn serialize_group(group: dao::Group, members: Vec<dao::User>) -> Group {
    Group {
        id: group.id.to_string(),
        name: group.name,
        members: members.into_iter().map(|it| it.into()).collect(),
    }
}

impl From<dao::Group> for GroupListItem {
    fn from(value: dao::Group) -> Self {
        GroupListItem {
            id: value.id.to_string(),
            name: value.name,
        }
    }
}

impl From<dao::User> for User {
    fn from(value: dao::User) -> Self {
        User {
            id: value.id,
            email: value.email,
            first_name: value.first_name,
            last_name: value.last_name,
            username: value.username,
        }
    }
}

impl From<dao::Game> for Game {
    fn from(value: dao::Game) -> Self {
        let status_str = match value.status {
            dao::GameStatus::Scheduled => "scheduled",
            dao::GameStatus::InProgress => "in_progress",
            dao::GameStatus::Completed => "completed",
            dao::GameStatus::Cancelled => "cancelled",
        };

        let game_type_enum = match value.game_type {
            dao::GameType::Football5ASide => GameType::Football5ASide,
            dao::GameType::Football11ASide => GameType::Football11ASide,
            dao::GameType::Basketball => GameType::Basketball,
            dao::GameType::Tennis => GameType::Tennis,
            dao::GameType::Badminton => GameType::Badminton,
            dao::GameType::Cricket => GameType::Cricket,
            dao::GameType::Rugby => GameType::Rugby,
            dao::GameType::Hockey => GameType::Hockey,
            dao::GameType::Other => GameType::Other,
        };

        Game {
            id: value.id,
            title: value.title,
            game_type: game_type_enum,
            location: Location {
                latitude: value.location_latitude.to_f64().unwrap_or(0.0),
                longitude: value.location_longitude.to_f64().unwrap_or(0.0),
                name: value.location_name,
            },
            scheduled_time: DateTime::from_naive_utc_and_offset(value.scheduled_time, Utc),
            duration_minutes: value.duration_minutes,
            created_by_user_id: value.created_by_user_id,
            created_at: DateTime::from_naive_utc_and_offset(value.created_at, Utc),
            status: status_str.to_string(),
        }
    }
}

impl From<dao::GameInvitation> for GameInvitation {
    fn from(value: dao::GameInvitation) -> Self {
        let status = match value.status {
            dao::InvitationStatus::Pending => InvitationStatus::Pending,
            dao::InvitationStatus::Accepted => InvitationStatus::Accepted,
            dao::InvitationStatus::Declined => InvitationStatus::Declined,
        };

        GameInvitation {
            game_id: value.game_id,
            user_id: value.user_id,
            team_id: value.team_id,
            status,
            invited_at: DateTime::from_naive_utc_and_offset(value.invited_at, Utc),
            responded_at: value
                .responded_at
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc)),
        }
    }
}

#[derive(Object)]
struct CreateGroupInput {
    name: String,
}

#[derive(Object)]
struct CreateUserInput {
    email: String,
    first_name: String,
    last_name: String,
    username: String,
}

#[derive(Object)]
struct AddGroupMembersInput {
    user_ids: Vec<String>,
}

#[derive(Object)]
struct Location {
    latitude: f64,
    longitude: f64,
    name: Option<String>,
}

#[derive(Object)]
struct CreateGameTeamInput {
    name: String,
    color: Option<String>,
    invited_user_ids: Vec<String>,
}

#[derive(Object)]
struct CreateGameInput {
    title: String,
    game_type: GameType,
    location: Location,
    scheduled_time: DateTime<Utc>,
    duration_minutes: i32,
    teams: Vec<CreateGameTeamInput>, // Can be 1 team (casual) or multiple teams
}

#[derive(Object)]
struct Game {
    id: String,
    title: String,
    game_type: GameType,
    location: Location,
    scheduled_time: DateTime<Utc>,
    duration_minutes: i32,
    created_by_user_id: String,
    created_at: DateTime<Utc>,
    status: String,
}

#[derive(Object)]
struct GameTeam {
    id: String,
    name: String,
    color: Option<String>,
    position: i32,
    members: Vec<User>,
}

#[derive(Object)]
struct GameInvitation {
    game_id: String,
    user_id: String,
    team_id: String,
    status: InvitationStatus,
    invited_at: DateTime<Utc>,
    responded_at: Option<DateTime<Utc>>,
}

#[derive(Object)]
struct GameWithInvitations {
    game: Game,
    teams: Vec<GameTeam>,
    invitations: Vec<GameInvitationWithUser>,
}

#[derive(Object)]
struct GameInvitationWithUser {
    user: User,
    invitation: GameInvitation,
}

#[derive(Object)]
struct RespondToInvitationInput {
    response: InvitationResponse,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum InvitationResponse {
    Accepted,
    Declined,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum GameStatus {
    Scheduled,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
enum GameType {
    #[oai(rename = "football_5_a_side")]
    Football5ASide,
    #[oai(rename = "football_11_a_side")]
    Football11ASide,
    Basketball,
    Tennis,
    Badminton,
    Cricket,
    Rugby,
    Hockey,
    Other,
}

#[derive(ApiResponse)]
enum GetGroupResponse {
    #[oai(status = 200)]
    Group(Json<Group>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[derive(ApiResponse)]
enum GetUserResponse {
    #[oai(status = 200)]
    User(Json<User>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[OpenApi]
impl Api {
    #[oai(path = "/ping", method = "get")]
    async fn ping(&self) -> Result<PlainText<String>> {
        Ok(PlainText("Pong".to_string()))
    }

    #[oai(path = "/users/me", method = "get")]
    async fn get_current_user(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<GetUserResponse> {
        info!("Getting current user");

        let user = dao
            .get_user(&jwt_data.sub)
            .await
            .map_err(InternalServerError)?;

        match user {
            Some(user) => Ok(GetUserResponse::User(Json(user.into()))),
            None => Ok(GetUserResponse::NotFound(PlainText(
                "User not found".to_string(),
            ))),
        }
    }

    #[oai(path = "/users", method = "post")]
    async fn create_user(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateUserInput>,
    ) -> Result<Json<User>> {
        let user = dao
            .create_user(
                jwt_data.sub,
                input.email.clone(),
                input.first_name.clone(),
                input.last_name.clone(),
                input.username.clone(),
            )
            .await
            .map_err(InternalServerError)?;

        Ok(Json(user.into()))
    }

    #[oai(path = "/users/search", method = "get")]
    async fn search_users(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        #[oai(name = "q")] Query(query): Query<String>,
    ) -> Result<Json<Vec<User>>> {
        info!("Searching users with query: {}", query);

        let users = dao
            .search_users(&query)
            .await
            .map_err(InternalServerError)?;

        Ok(Json(users.into_iter().map(|u| u.into()).collect()))
    }

    #[oai(path = "/groups", method = "post")]
    async fn create_group(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateGroupInput>,
    ) -> Result<Json<Group>> {
        let group = dao
            .create_group(jwt_data.sub, input.name.clone())
            .await
            .map_err(InternalServerError)?;

        let group_members = dao
            .list_group_members(&group.id)
            .await
            .map_err(InternalServerError)?;

        Ok(Json(serialize_group(group, group_members)))
    }

    #[oai(path = "/groups", method = "get")]
    async fn list_groups(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<Json<Vec<GroupListItem>>> {
        info!("Listing groups");
        let groups = dao
            .list_user_groups(jwt_data.sub)
            .await
            .map_err(InternalServerError)?;
        info!("Listed groups");

        Ok(Json(groups.into_iter().map(|g| g.into()).collect()))
    }

    #[oai(path = "/groups/:id", method = "get")]
    async fn get_group(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(id): Path<String>,
    ) -> Result<GetGroupResponse> {
        info!("Getting group");

        let group = dao
            .get_user_group(jwt_data.sub, id.clone())
            .await
            .map_err(InternalServerError)?;

        info!("Got group");

        let group_members = dao.list_group_members(&id).await.map_err(|e| {
            error!("Failed to list group members {:?}", e);
            InternalServerError(e)
        })?;

        info!("Got group members");

        Ok(match group {
            Some(group) => GetGroupResponse::Group(Json(serialize_group(group, group_members))),
            None => GetGroupResponse::NotFound(PlainText("Group not found".to_string())),
        })
    }

    #[oai(path = "/groups/:group_id/members", method = "post")]
    async fn add_group_members(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(group_id): Path<String>,
        Json(input): Json<AddGroupMembersInput>,
    ) -> Result<()> {
        // TODO Handle if user ids don't exists (postgres should throw an error already just need
        // to handle it)

        // TODO Validate caller is admin member of group

        for user_id in input.user_ids {
            dao.add_user_to_group(&group_id, &user_id)
                .await
                .map_err(InternalServerError)?;
        }

        Ok(())
    }

    #[oai(path = "/games", method = "post")]
    async fn create_game(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateGameInput>,
    ) -> Result<Json<Game>> {
        info!("Creating game ");

        // Convert DateTime<Utc> to NaiveDateTime for the DAO layer
        let scheduled_time = input.scheduled_time.naive_utc();

        // Convert API game type to DAO game type
        let dao_game_type = match input.game_type {
            GameType::Football5ASide => dao::GameType::Football5ASide,
            GameType::Football11ASide => dao::GameType::Football11ASide,
            GameType::Basketball => dao::GameType::Basketball,
            GameType::Tennis => dao::GameType::Tennis,
            GameType::Badminton => dao::GameType::Badminton,
            GameType::Cricket => dao::GameType::Cricket,
            GameType::Rugby => dao::GameType::Rugby,
            GameType::Hockey => dao::GameType::Hockey,
            GameType::Other => dao::GameType::Other,
        };

        // Prepare teams data for transactional creation
        let teams_input: Vec<dao::CreateGameTeamInput> = input
            .teams
            .iter()
            .enumerate()
            .map(|(index, team_input)| dao::CreateGameTeamInput {
                name: team_input.name.clone(),
                color: team_input.color.clone(),
                position: (index + 1) as i32,
                invited_user_ids: team_input.invited_user_ids.clone(),
            })
            .collect();

        // Create DAO input struct
        let dao_input = dao::CreateGameInput {
            created_by_user_id: jwt_data.sub.clone(),
            title: input.title.clone(),
            game_type: dao_game_type,
            location_latitude: BigDecimal::from_f64(input.location.latitude).ok_or_else(|| {
                Error::from_string("Invalid latitude", StatusCode::BAD_REQUEST)
            })?,
            location_longitude: BigDecimal::from_f64(input.location.longitude).ok_or_else(|| {
                Error::from_string("Invalid longitude", StatusCode::BAD_REQUEST)
            })?,
            location_name: input.location.name.clone(),
            scheduled_time,
            duration_minutes: input.duration_minutes,
            teams: teams_input,
        };

        // Create game, teams, and invitations in a single transaction
        let game = dao
            .create_game(dao_input)
            .await
            .map_err(InternalServerError)?;

        Ok(Json(game.into()))
    }

    #[oai(path = "/games", method = "get")]
    async fn list_games(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<Json<Vec<Game>>> {
        info!("Listing games for user");

        let games = dao
            .list_user_games(&jwt_data.sub)
            .await
            .map_err(InternalServerError)?;

        Ok(Json(games.into_iter().map(|g| g.into()).collect()))
    }

    #[oai(path = "/games/:id", method = "get")]
    async fn get_game(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(id): Path<String>,
    ) -> Result<Json<GameWithInvitations>> {
        info!("Getting game details");

        let result = dao
            .get_game_with_invitations(&id)
            .await
            .map_err(InternalServerError)?;

        match result {
            Some((game, user_invitations)) => {
                // Get teams for this game
                let teams_data = dao
                    .list_game_teams(&id)
                    .await
                    .map_err(InternalServerError)?;

                // Build teams with their members
                let teams = teams_data.into_iter().map(|team_data| {
                    let team_members: Vec<User> = user_invitations
                        .iter()
                        .filter(|(_, invitation)| invitation.team_id == team_data.id)
                        .map(|(user, _)| (*user).clone().into())
                        .collect();

                    GameTeam {
                        id: team_data.id.clone(),
                        name: team_data.name,
                        color: team_data.color,
                        position: team_data.position,
                        members: team_members,
                    }
                }).collect();

                let invitations = user_invitations
                    .into_iter()
                    .map(|(user, invitation)| GameInvitationWithUser {
                        user: user.into(),
                        invitation: invitation.into(),
                    })
                    .collect();

                Ok(Json(GameWithInvitations {
                    game: game.into(),
                    teams,
                    invitations,
                }))
            }
            None => Err(Error::from_string("Game not found", StatusCode::NOT_FOUND)),
        }
    }

    #[oai(path = "/games/:game_id/invitations", method = "post")]
    async fn add_game_invitations(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(_jwt_data): AuthSchema,
        Path(game_id): Path<String>,
        input: Json<AddGroupMembersInput>, // Reuse the same input type
    ) -> Result<()> {
        info!("Adding invitations to game {}", game_id);

        dao.add_game_invitations(&game_id, &input.user_ids)
            .await
            .map_err(InternalServerError)?;

        Ok(())
    }

    #[oai(path = "/games/:game_id/invitations/:user_id", method = "put")]
    async fn respond_to_invitation(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(game_id): Path<String>,
        Path(user_id): Path<String>,
        input: Json<RespondToInvitationInput>,
    ) -> Result<()> {
        info!("Responding to game invitation");

        // Ensure the user can only respond for themselves
        if jwt_data.sub != user_id {
            return Err(Error::from_string("Unauthorized", StatusCode::FORBIDDEN));
        }

        let response_enum = match input.response {
            InvitationResponse::Accepted => dao::InvitationStatus::Accepted,
            InvitationResponse::Declined => dao::InvitationStatus::Declined,
        };

        dao.respond_to_game_invitation(&game_id, &user_id, response_enum)
            .await
            .map_err(InternalServerError)?;

        Ok(())
    }
}

async fn create_dao() -> Result<Dao, sqlx::Error> {
    let db_url = std::env::var("DATABASE_URL").expect("Database url must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    let dao = Dao::create(pool);

    Ok(dao)
}

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "git")]
#[command(about = "Agon Service CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Starts the service
    #[command(arg_required_else_help = true)]
    RunServer {
        /// The url of the service
        url: String,
    },

    /// Generates service open api schema
    GenerateSchema,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let args = Cli::parse();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:7000");

    match args.command {
        Commands::RunServer { url: _ } => {
            info!("Starting up server");

            let ui = api_service.swagger_ui();

            let dao = create_dao().await.unwrap();

            let cors = Cors::new()
                .allow_origin("http://localhost:5173")
                .allow_origin("http://localhost:5174")
                .allow_origin("http://localhost:5175")
                .allow_origin("http://localhost:3000")
                .allow_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
                .allow_headers(vec!["content-type", "authorization"])
                .allow_credentials(true);

            let app = Route::new()
                .nest("/", api_service)
                .nest("/docs", ui)
                .with(cors)
                .data(dao);

            Server::new(TcpListener::bind("0.0.0.0:7000"))
                .run(app)
                .await
                .expect("Failed to start server");
        }

        Commands::GenerateSchema => {
            let mut file = File::create("schema.json").expect("Cannot create schema/schmea.json");
            file.write_all(api_service.spec().as_bytes())
                .expect("Failed to write to file");
        }
    }
}
