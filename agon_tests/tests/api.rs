use jsonwebtoken::{EncodingKey, Header, encode};
use openapi::apis::configuration::{self, Configuration};
use openapi::apis::default_api::{teams_get, teams_id_get, teams_post, teams_team_id_members_post, users_post};
use openapi::models::{AddTeamMembersInput, CreateTeamInput, CreateUserInput, Team, User};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

struct TestResources {
    configuration: Configuration,
    user: User,
    user2: User,
}

static TEST_RESOURCES: OnceCell<TestResources> = OnceCell::const_new();

async fn get_test_resources() -> &'static TestResources {
    TEST_RESOURCES.get_or_init(|| async {
        println!("Initializing tests");

        let configuration = get_configuration();
        let user = create_user(CreateUserInput::default(), &configuration).await;
        let user2 = create_user(CreateUserInput::default(), &get_user_2_configuration()).await;

        TestResources {
            configuration,
            user,
            user2,
        }
    })
    .await
}

#[derive(Debug, Deserialize, Serialize)]
struct JwtData {
    sub: String,
    exp: usize,
}

fn generate_jwt() -> String {
    let my_claims = JwtData {
        sub: Uuid::new_v4().to_string(),
        exp: 9999999999,
    };

    let secret_key = std::env::var("SUPABASE_JWT_SECRET").expect("JWT Secret not found");
    encode(
        &Header::default(),
        &my_claims,
        &EncodingKey::from_secret(secret_key.as_bytes()),
    )
    .expect("Failed to generate test jwt")
}

fn get_configuration() -> Configuration {
    dotenv::dotenv().ok();
    Configuration {
        bearer_access_token: Some(generate_jwt()),
        ..Default::default()
    }
}

fn get_user_2_configuration() -> Configuration {
    dotenv::dotenv().ok();
    Configuration {
        bearer_access_token: Some(generate_jwt()),
        ..Default::default()
    }
}

async fn create_user(input: CreateUserInput, configuration: &Configuration) -> User {
    let response = users_post(&configuration, input).await;
    assert!(response.is_ok());
    response.unwrap()
}

async fn create_team(input: CreateTeamInput, configuration: &Configuration) -> Team {
    let response = teams_post(&configuration, input).await;
    assert!(response.is_ok());
    response.unwrap()
}

#[tokio::test]
async fn my_test() {
    let test_resource = get_test_resources().await;
    create_team(CreateTeamInput::default(), &test_resource.configuration).await;

    let result = teams_get(&test_resource.configuration).await;
    dbg!(&result);
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.len() >= 1);
    let team = result.first().unwrap();
    // assert_eq!(team.name, "My awesome team");
}

#[tokio::test]
async fn get_returns_not_found() {
    let test_resource = get_test_resources().await;
    let id = "some-fake-id";

    let response = teams_id_get(&test_resource.configuration, id).await;

    assert!(response.is_err());

    let err = response.unwrap_err();
    assert!(matches!(
        err,
        openapi::apis::Error::ResponseError(openapi::apis::ResponseContent {
            status: reqwest::StatusCode::NOT_FOUND,
            content: _,
            // TODO This seems strange?
            // entity: Some(TeamsIdGetError::Status404(_))
            entity: None,
        })
    ));
}

#[tokio::test]
async fn get_returns_team() {
    let test_resources = get_test_resources().await;
    let team = create_team(CreateTeamInput {
        name: "Some team name".to_string(),
    }, &test_resources.configuration).await;

    let response = teams_id_get(&test_resources.configuration, &team.id).await;

    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.name, "Some team name");
}

#[tokio::test]
async fn team_members() {
    let test_resources = get_test_resources().await;

    let team = create_team(CreateTeamInput {
        name: "Some team name".to_string(),
    }, &test_resources.configuration).await;

    let response = teams_team_id_members_post(
        &test_resources.configuration,
        &team.id,
        AddTeamMembersInput { 
            user_ids: vec![test_resources.user2.id.clone()],
        },
    ).await;

    assert!(response.is_ok());

    let response = teams_id_get(&test_resources.configuration, &team.id).await;

    assert!(response.is_ok());
    let response = response.unwrap();

    vec![test_resources.user.id.clone(), test_resources.user2.id.clone()].iter().for_each(|user_id| {
        assert!(response.members.iter().find(|user| &user.id == user_id).is_some())
    });
}
