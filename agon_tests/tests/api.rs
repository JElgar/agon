use jsonwebtoken::{EncodingKey, Header, encode};
use openapi::apis::configuration::{self, Configuration};
use openapi::apis::default_api::{
    groups_get, groups_group_id_members_post, groups_id_get, groups_post, users_post,
};
use openapi::models::{AddGroupMembersInput, CreateGroupInput, CreateUserInput, Group, User};
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
                CreateUserInput {
                    username: user_id.clone(),
                    ..CreateUserInput::default()
                },
                &get_configuration_for_user(&user_id),
            )
            .await;

            let user2_id = Uuid::new_v4().to_string();
            let user2 = create_user(
                CreateUserInput {
                    username: user2_id.clone(),
                    ..CreateUserInput::default()
                },
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
    Configuration {
        base_path: std::env::var("AGON_SERVICE_URL").expect("AGON_SERVICE_URL must be set"),
        bearer_access_token: Some(generate_jwt(user_id)),
        ..Default::default()
    }
}

async fn create_user(input: CreateUserInput, configuration: &Configuration) -> User {
    let response = users_post(&configuration, input).await;
    dbg!(&response);
    assert!(response.is_ok());
    response.unwrap()
}

async fn create_group(input: CreateGroupInput, configuration: &Configuration) -> Group {
    let response = groups_post(&configuration, input).await;
    dbg!(&response);
    assert!(response.is_ok());
    response.unwrap()
}

#[tokio::test]
async fn my_test() {
    let test_resource = get_test_resources().await;
    let configuration = get_configuration_for_user(&test_resource.user.id);

    create_group(CreateGroupInput::default(), &configuration).await;

    let result = groups_get(&configuration).await;
    dbg!(&result);
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.len() >= 1);
    let group = result.first().unwrap();
    // assert_eq!(group.name, "My awesome group");
}

#[tokio::test]
async fn get_returns_not_found() {
    let test_resource = get_test_resources().await;
    let id = "some-fake-id";
    let configuration = get_configuration_for_user(&test_resource.user.id);

    let response = groups_id_get(&configuration, id).await;

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
async fn get_returns_group() {
    let test_resources = get_test_resources().await;
    let configuration = get_configuration_for_user(&test_resources.user.id);

    let group = create_group(
        CreateGroupInput {
            name: "Some group name".to_string(),
        },
        &configuration,
    )
    .await;

    let response = groups_id_get(&configuration, &group.id).await;

    assert!(response.is_ok());
    let response = response.unwrap();
    assert_eq!(response.name, "Some group name");
}

#[tokio::test]
async fn group_members() {
    let test_resources = get_test_resources().await;
    let configuration = get_configuration_for_user(&test_resources.user.id);

    let group = create_group(
        CreateGroupInput {
            name: "Some group name".to_string(),
        },
        &configuration,
    )
    .await;

    let response = groups_group_id_members_post(
        &configuration,
        &group.id,
        AddGroupMembersInput {
            user_ids: vec![test_resources.user2.id.clone()],
        },
    )
    .await;

    dbg!(&response);
    assert!(response.is_ok());

    let response = groups_id_get(&configuration, &group.id).await;

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
