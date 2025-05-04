use dao::Dao;
use poem::{
    EndpointExt, Result, Route, Server, error::InternalServerError, listener::TcpListener,
    web::Data,
};
use poem_openapi::{
    ApiResponse, Object, OpenApi, OpenApiService,
    param::Path,
    payload::{Json, PlainText},
};
use surrealdb::{engine::remote::ws::{Client, Ws}, opt::auth::Root, Surreal};
use uuid::Uuid;

mod dao;

const TABLE_NAME: &'static str = "AgonTable";

struct Api;

#[derive(Object)]
struct User {
    id: String,
    email: String,
    first_name: String,
    last_name: String,
}

#[derive(Object)]
struct Team {
    id: String,
    name: String,
    members: Vec<User>,
}

impl From<dao::Team> for Team {
    fn from(value: dao::Team) -> Self {
        Team {
            id: value.id,
            name: value.name,
            members: vec![],
        }
    }
}

#[derive(Object)]
struct CreateTeamInput {
    name: String,
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
    #[oai(path = "/teams", method = "post")]
    async fn create_team(
        &self,
        Data(dao): Data<&Dao>,
        input: Json<CreateTeamInput>,
    ) -> Result<Json<Team>> {
        let team = dao.create_team(
            input.name.clone(),
            "someuser".into(),
        ).await.map_err(InternalServerError)?;

        Ok(Json(team.into()))
    }

    #[oai(path = "/teams", method = "get")]
    async fn list_teams(&self, Data(dao): Data<&Dao>) -> Result<Json<Vec<Team>>> {
        dao.list_user_teams("someuser".to_string()).await.map_err(InternalServerError)?;

        let teams = vec![Team {
            id: Uuid::new_v4().to_string(),
            name: "Some name".to_string(),
            members: vec![],
        }];

        Ok(Json(teams))
    }

    #[oai(path = "/teams/:id", method = "get")]
    async fn get_team(
        &self,
        pool: Data<&Dao>,
        Path(id): Path<String>,
    ) -> Result<GetTeamResponse> {
        Ok(GetTeamResponse::Team(Json(Team {
            id,
            name: "Some name".to_string(),
            members: vec![],
        })))
    }
}

async fn create_dao() -> Result<Dao, surrealdb::Error> {
    let db: Surreal<Client> = Surreal::init();

    db.connect::<Ws>("localhost:8000").await?;
    db.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;
    db.use_ns("test").use_db("test").await?;

    db.query("CREATE user:someuser").await?;
//     db.query(
//         "
// DEFINE TABLE IF NOT EXISTS user SCHEMALESS
//     PERMISSIONS FOR
//         CREATE, SELECT WHERE $auth,
//         FOR UPDATE, DELETE WHERE created_by = $auth;
// DEFINE FIELD IF NOT EXISTS name ON TABLE person TYPE string;
// DEFINE FIELD IF NOT EXISTS created_by ON TABLE person VALUE $auth READONLY;
// 
// DEFINE INDEX IF NOT EXISTS unique_name ON TABLE user FIELDS name UNIQUE;
// DEFINE ACCESS IF NOT EXISTS account ON DATABASE TYPE RECORD
// SIGNUP ( CREATE user SET name = $name, pass = crypto::argon2::generate($pass) )
// SIGNIN ( SELECT * FROM user WHERE name = $name AND crypto::argon2::compare(pass, $pass) )
// DURATION FOR TOKEN 15m, FOR SESSION 12h
// ;",
//     )
//    .await?;

    let dao = Dao::create(db);
    return Ok(dao);
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:3000");
    let ui = api_service.swagger_ui();

    let dao = create_dao().await.unwrap();

    let app = Route::new()
        .nest("/", api_service)
        .nest("/docs", ui)
        .data(dao);

    Server::new(TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await;
}
