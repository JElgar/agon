use aws_sdk_dynamodb::operation::transact_write_items;
use aws_sdk_dynamodb::Client;
use aws_sdk_dynamodb::types::{AttributeValue, KeySchemaElement};
use uuid::Uuid;

const TABLE_NAME: &'static str = "AgonTable";

pub enum DaoError {
    InternalServerError,
}

struct Team {
    id: String,
    name: String,
}

struct User {
    id: String,
    email: String,
    first_name: String,
    last_name: String,
}

// PK=TEAM#team_id SK=TEAM
// Primary index -> list team members - GSI1 -> List teams user is a member of
// PK=TEAM#team_id SK=USER#user_id GSI1_PK=USER#user_id GSI1_SK=TEAM#team_id

pub async fn create_team(
    name: String,
    client: &Client,
) -> Result<Team, DaoError> {
    let team = Team {
        id: Uuid::new_v4().to_string(),
        name,
    };

    let request = client
        .put_item()
        .table_name(TABLE_NAME)
        .item("PK", AttributeValue::S(format!("TEAM#{}", team.id)))
        .item("SK", AttributeValue::S("TEAMS".to_string()))
        .item("id", AttributeValue::S(team.id.clone()))
        .item("name", AttributeValue::S(team.name.clone()));

    let request = transact_write_items()

    request.send().await.map_err(|_| DaoError::InternalServerError)?;

    Ok(team)
}

fn list_team_members(
    team_id: String,
    client: &Client,
) -> Vec<User> {
    let request = client
        .query()
        .table_name(TABLE_NAME)
        .key_condition_expression("#pk = :pk")
        .expression_attribute_names("#pk", "PK")
        .expression_attribute_values(":pk", );

    todo!()
}
