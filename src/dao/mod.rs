use id::{SurrealId, Table};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::{RecordId, Surreal, engine::remote::ws::Client};
use thiserror::Error;
use uuid::Uuid;

mod id;

const USER_TAG: &'static str = "user";
const TEAM_TAG: &'static str = "team";

#[derive(Clone, Debug)]
struct UserTable;
impl Table for UserTable {
    fn table_name() -> &'static str {
        return USER_TAG;
    }
}

#[derive(Clone, Debug)]
struct TeamTable;
impl Table for TeamTable {
    fn table_name() -> &'static str {
        return TEAM_TAG;
    }
}

#[derive(Error, Debug)]
pub enum DaoError {
    #[error("internal error")]
    InternalServerError(String),
}

pub struct Team {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TeamContent {
    #[serde(skip_serializing)]
    id: SurrealId<TeamTable>,
    name: String,
    created_by_user_id: SurrealId<UserTable>,
}

pub struct User {
    pub id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
}

impl From<surrealdb::Error> for DaoError {
    fn from(error: surrealdb::Error) -> Self {
        eprintln!("{error}");
        DaoError::InternalServerError(error.to_string())
    }
}

// PK=TEAM#team_id SK=TEAM
// Primary index -> list team members - GSI1 -> List teams user is a member of
// PK=TEAM#team_id SK=USER#user_id GSI1_PK=USER#user_id GSI1_SK=TEAM#team_id

#[derive(Clone)]
pub struct Dao {
    client: Surreal<Client>,
}

#[derive(Deserialize)]
struct UserTeamsListResponse {
    #[serde(rename = "->member->team")]
    teams: Vec<TeamContent>,
}

impl Dao {
    pub fn create(client: Surreal<Client>) -> Self {
        Self { client }
    }

    pub async fn create_team(
        &self,
        name: String,
        created_by_user_id: String,
    ) -> Result<Team, DaoError> {
        let team = Team {
            id: Uuid::new_v4().to_string(),
            name,
        };

        println!("Creating team id={}", team.id);

        let team_content = TeamContent {
            id: SurrealId::new(&team.id.clone()),
            name: team.name.clone(),
            created_by_user_id: SurrealId::new(&created_by_user_id),
        };

        let team_content_json = serde_json::to_value(&team_content).map_err(|err| {
            // TODO Log error
            DaoError::InternalServerError("Failed to serialize team content".to_string())
        })?;

        let query = r#"
            BEGIN TRANSACTION;
            CREATE $team_id CONTENT $team_data;
            RELATE $user_id -> member -> $team_id CONTENT { joined_at: time::now() };
            COMMIT TRANSACTION;
        "#;

        let response = self
            .client
            .query(query)
            .bind(("team_id", RecordId::from((TEAM_TAG, &team.id))))
            .bind(("user_id", RecordId::from((USER_TAG, &created_by_user_id))))
            .bind(("team_data", team_content_json))
            .await?;

        match response.check() {
            Err(err) => println!("Error in the query {}", err),
            Ok(_) => println!("Query all good"),
        }

        Ok(team)
    }

    pub async fn list_user_teams(&self, user_id: String) -> Result<Vec<Team>, DaoError> {
        println!("Listing user teams user_id={}", user_id);
        let query = "SELECT ->member->team.* FROM $user_id;";
        let response: Option<serde_json::Value> = self
            .client
            .query(query)
            .bind(("user_id", RecordId::from((USER_TAG, &user_id))))
            .await?
            .take(0)?;

        dbg!("{:?}", response)

        todo!()

        // let teams: Vec<TeamContent> = response
        //             .get("->member")
        //             .and_then(|member| member.get("->team"))
        //             .and_then(|teams| teams.as_array())
        //             .unwrap_or(&vec![])
        //             .iter()
        //             .filter_map(|team| serde_json::from_value::<TeamContent>(team.clone()).ok())
        //             .collect::<Vec<TeamContent>>()

        // println!("Parsing response");

        // let teams: Vec<TeamContent> = response
        //     .into_iter()
        //     .filter_map(|entry| entry.get("->member")?.get("->team")?.as_array().cloned())
        //     .flatten()
        //     .map(|value| {
        //         println!("Parsing {}", value);
        //         serde_json::from_value(value)
        //     })
        //     .collect::<std::result::Result<Vec<TeamContent>, _>>()
        //     .map_err(|err| {
        //         // TODO Log
        //         DaoError::InternalServerError("Failed to deserialize results".to_string())
        //     })?;

        // Ok(teams.into_iter().map(|team| Team {
        //     id: team.id.id().to_string(),
        //     name: team.name,
        // }).collect())
    }
}
