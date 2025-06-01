use jsonwebtoken::{EncodingKey, Header, encode};
use openapi::apis::configuration::{self, Configuration};
use openapi::apis::default_api::{
    teams_get, teams_id_get, teams_post, teams_team_id_members_post, users_post,
};
use openapi::models::{AddTeamMembersInput, CreateTeamInput, CreateUserInput, Team, User};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use uuid::Uuid;

struct TestResources {
    user: User,
    user2: User,
}

static TEST_RESOURCES: OnceCell<TestResources> = OnceCell::const_new();

async fn get_test_resources() -> &'static TestResources {
    TEST_RESOURCES
        .get_or_init(|| async {
            println!("Initializing tests");

            let user_id = Uuid::new_v4().to_string();
            let user = create_user(
                CreateUserInput::default(),
                &get_configuration_for_user(&user_id),
            )
            .await;

            let user2_id = Uuid::new_v4().to_string();
            let user2 = create_user(
                CreateUserInput::default(),
                &get_configuration_for_user(&user2_id),
            )
            .await;

            TestResources { user, user2 }
        })
        .await
}

#[derive(Debug, Deserialize, Serialize)]
struct JwtData {
    sub: String,
    exp: usize,
}

fn generate_jwt(user_id: &String) -> String {
    let my_claims = JwtData {
        sub: user_id.clone(),
        exp: 9999999999,
    };

    let secret_key = std::env::var("JWT_SECRET").expect("JWT Secret not found");
    encode(
        &Header::default(),
        &my_claims,
        &EncodingKey::from_secret(secret_key.as_bytes()),
    )
    .expect("Failed to generate test jwt")
}

fn get_configuration_for_user(user_id: &String) -> Configuration {
    dotenv::dotenv().ok();
    Configuration {
        bearer_access_token: Some(generate_jwt(user_id)),
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
    dbg!(&response);
    assert!(response.is_ok());
    response.unwrap()
}

#[tokio::test]
async fn my_test() {
    let test_resource = get_test_resources().await;
    let configuration = get_configuration_for_user(&test_resource.user.id);

    create_team(CreateTeamInput::default(), &configuration).await;

    let result = teams_get(&configuration).await;
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
    let configuration = get_configuration_for_user(&test_resource.user.id);

    let response = teams_id_get(&configuration, id).await;

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
    let configuration = get_configuration_for_user(&test_resources.user.id);

    let team = create_team(
        CreateTeamInput {
            name: "Some team name".to_string(),
        },
        &configuration,
    )
    .await;

    let response = teams_id_get(&configuration, &team.id).await;

    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.name, "Some team name");
}

#[tokio::test]
async fn team_members() {
    let test_resources = get_test_resources().await;
    let configuration = get_configuration_for_user(&test_resources.user.id);

    let team = create_team(
        CreateTeamInput {
            name: "Some team name".to_string(),
        },
        &configuration,
    )
    .await;

    let response = teams_team_id_members_post(
        &configuration,
        &team.id,
        AddTeamMembersInput {
            user_ids: vec![test_resources.user2.id.clone()],
        },
    )
    .await;

    dbg!(&response);
    assert!(response.is_ok());

    let response = teams_id_get(&configuration, &team.id).await;

    assert!(response.is_ok());
    let response = response.unwrap();

    vec![
        test_resources.user.id.clone(),
        test_resources.user2.id.clone(),
    ]
    .iter()
    .for_each(|user_id| {
        assert!(
            response
                .members
                .iter()
                .find(|user| &user.id == user_id)
                .is_some()
        )
    });
}
