use std::{fs::File, io::Write};

use clap::{Parser, Subcommand};
use dao::Dao;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use poem::{
    EndpointExt, Error, FromRequest, Request, RequestBody, Result, Route, Server,
    error::InternalServerError, http::StatusCode, listener::TcpListener, web::Data,
};
use poem_openapi::auth::{Bearer, BearerAuthorization};
use poem_openapi::{
    ApiResponse, Object, OpenApi, OpenApiService, SecurityScheme,
    param::Path,
    payload::{Json, PlainText},
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info};
use tracing_subscriber;
use uuid::Uuid;

mod dao;

#[derive(Debug, Deserialize, Serialize)]
struct JwtClaims {
    sub: String,
    exp: usize,
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
    // Change to change the validity of the token (set to false to fail the validation)
    let secret_key = std::env::var("SUPABASE_JWT_SECRET").expect("JWT Secret not found");
    let decoding_key = DecodingKey::from_secret(secret_key.as_bytes());

    let token_data = decode::<JwtClaims>(
        &bearer.token,
        &decoding_key,
        &Validation::new(Algorithm::HS256),
    )
    .map_err(|err| {
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
}

#[derive(Object)]
struct TeamListItem {
    id: String,
    name: String,
}

#[derive(Object)]
struct Team {
    id: String,
    name: String,
    members: Vec<User>,
}

fn serialize_team(team: dao::Team, members: Vec<dao::User>) -> Team {
    Team {
        id: team.id.to_string(),
        name: team.name,
        members: members.into_iter().map(|it| it.into()).collect(),
    }
}

impl From<dao::Team> for TeamListItem {
    fn from(value: dao::Team) -> Self {
        TeamListItem {
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
        }
    }
}

#[derive(Object)]
struct CreateTeamInput {
    name: String,
}

#[derive(Object)]
struct CreateUserInput {
    email: String,
    first_name: String,
    last_name: String,
}

#[derive(Object)]
struct AddTeamMembersInput {
    user_ids: Vec<String>,
}

#[derive(ApiResponse)]
enum GetTeamResponse {
    #[oai(status = 200)]
    Team(Json<Team>),

    #[oai(status = 404)]
    NotFound(PlainText<String>),
}

#[OpenApi]
impl Api {
    #[oai(path = "/ping", method = "get")]
    async fn ping(&self) -> Result<PlainText<String>> {
        Ok(PlainText("Pong".to_string()))
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
            )
            .await
            .map_err(InternalServerError)?;

        Ok(Json(user.into()))
    }

    #[oai(path = "/teams", method = "post")]
    async fn create_team(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        input: Json<CreateTeamInput>,
    ) -> Result<Json<Team>> {
        let team = dao
            .create_team(jwt_data.sub, input.name.clone())
            .await
            .map_err(InternalServerError)?;

        let team_members = dao
            .list_team_members(&team.id)
            .await
            .map_err(InternalServerError)?;

        Ok(Json(serialize_team(team, team_members)))
    }

    #[oai(path = "/teams", method = "get")]
    async fn list_teams(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
    ) -> Result<Json<Vec<TeamListItem>>> {
        info!("Listing teams");
        let teams = dao
            .list_user_teams(jwt_data.sub)
            .await
            .map_err(InternalServerError)?;
        info!("Listed teams");

        Ok(Json(teams.into_iter().map(|t| t.into()).collect()))
    }

    #[oai(path = "/teams/:id", method = "get")]
    async fn get_team(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(id): Path<String>,
    ) -> Result<GetTeamResponse> {
        info!("Getting team");

        let team = dao
            .get_user_team(jwt_data.sub, id.clone())
            .await
            .map_err(InternalServerError)?;

        info!("Got team");

        let team_members = dao
            .list_team_members(&id)
            .await
            .map_err(|e| {
                error!("Failed to list team members {:?}", e);
                InternalServerError(e)
            })?;

        info!("Got team members");

        Ok(match team {
            Some(team) => GetTeamResponse::Team(Json(serialize_team(team, team_members))),
            None => GetTeamResponse::NotFound(PlainText("Team not found".to_string())),
        })
    }

    #[oai(path = "/teams/:team_id/members", method = "post")]
    async fn add_team_members(
        &self,
        Data(dao): Data<&Dao>,
        AuthSchema(jwt_data): AuthSchema,
        Path(team_id): Path<String>,
        Json(input): Json<AddTeamMembersInput>,
    ) -> Result<()> {
        // TODO Handle if user ids don't exists (postgres should throw an error already just need
        // to handle it)

        // TODO Validate caller is admin member of team

        for user_id in input.user_ids {
            dao.add_user_to_team(&team_id, &user_id)
                .await
                .map_err(InternalServerError)?;
        }

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

    return Ok(dao);
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
    dotenv::dotenv().ok();

    let args = Cli::parse();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:7000");

    match args.command {
        Commands::RunServer { url } => {
            info!("Starting up server");

            let ui = api_service.swagger_ui();

            let dao = create_dao().await.unwrap();

            let app = Route::new()
                .nest("/", api_service)
                .nest("/docs", ui)
                .data(dao);

            Server::new(TcpListener::bind("127.0.0.1:7000"))
                .run(app)
                .await;
        }

        Commands::GenerateSchema => {
            let mut file = File::create("schema.json").expect("Cannot create schema/schmea.json");
            file.write_all(api_service.spec().as_bytes())
                .expect("Failed to write to file");
        }
    }
}
