use aws_sdk_dynamodb::{Client, Error};
use aws_sdk_dynamodb::types::AttributeValue;
use poem::{error::InternalServerError, EndpointExt, Result, Route, Server, listener::TcpListener, web::Data};
use poem_openapi::{
    Object, OpenApi, OpenApiService, ApiResponse,
    param::Path,
    payload::{Json, PlainText},
};
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
        Data(client): Data<&Client>,
        input: Json<CreateTeamInput>,
    ) -> Result<Json<Team>> {
        let team = Team {
            id: Uuid::new_v4().to_string(),
            name: input.name.clone(),
            members: vec![],
        };

        let request = client
            .put_item()
            .table_name(TABLE_NAME)
            .item("PK", AttributeValue::S(format!("TEAM#{}", team.id)))
            .item("id", AttributeValue::S(team.id.clone()))
            .item("name", AttributeValue::S(team.name.clone()));

        request.send().await.map_err(InternalServerError)?;

        Ok(Json(team))
    }

    #[oai(path = "/teams", method = "get")]
    async fn list_teams(&self, pool: Data<&Client>) -> Result<Json<Vec<Team>>> {
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
        pool: Data<&Client>,
        Path(id): Path<String>,
    ) -> Result<GetTeamResponse> {
        Ok(
            GetTeamResponse::Team(
                Json(
                    Team {
                        id,
                        name: "Some name".to_string(),
                        members: vec![],
                    }
                )
            )
        )
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let api_service =
        OpenApiService::new(Api, "Hello World", "1.0").server("http://localhost:3000");
    let ui = api_service.swagger_ui();

    let config = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&config);

    let app = Route::new()
        .nest("/", api_service)
        .nest("/docs", ui)
        .data(client);

    Server::new(TcpListener::bind("127.0.0.1:3000"))
        .run(app)
        .await;
}
