use std::str::FromStr;

use base64::{Engine, encode, prelude::BASE64_STANDARD};
use rand::Rng;
use sqlx::{Pool, Postgres, Transaction, query, query_as, types::time::OffsetDateTime};
use thiserror::Error;
use tracing::{error, info};

fn generate_id() -> String {
    let random_bytes: [u8; 8] = rand::rng().random();
    return BASE64_STANDARD.encode(&random_bytes);
}

#[derive(Error, Debug)]
pub enum DaoError {
    #[error("internal error")]
    InternalServerError(String),
}

pub struct Team {
    pub id: String,
    pub name: String,
    pub created_by_user_id: String,
    pub created_at: OffsetDateTime,
}

pub struct User {
    pub id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub created_at: OffsetDateTime,
}

// PK=TEAM#team_id SK=TEAM
// Primary index -> list team members - GSI1 -> List teams user is a member of
// PK=TEAM#team_id SK=USER#user_id GSI1_PK=USER#user_id GSI1_SK=TEAM#team_id

#[derive(Clone)]
pub struct Dao {
    pool: Pool<Postgres>,
}

impl Dao {
    pub fn create(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    pub async fn create_user(
        &self,
        sub: String,
        email: String,
        first_name: String,
        last_name: String,
    ) -> Result<User, DaoError> {
        let user = User {
            id: sub,
            email,
            first_name,
            last_name,
            created_at: OffsetDateTime::now_utc(),
        };

        query!(
            "INSERT INTO users (id, first_name, last_name, email, created_at)
            VALUES ($1, $2, $3, $4, $5)",
            user.id,
            user.first_name,
            user.last_name,
            user.email,
            user.created_at
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to insert user {:?}", err);
            DaoError::InternalServerError("Failed to insert user".to_string())
        })?;

        Ok(user)
    }

    pub async fn create_team(&self, user_id: String, name: String) -> Result<Team, DaoError> {
        info!("Creating team");

        let team = Team {
            id: generate_id(),
            name,
            created_by_user_id: user_id,
            created_at: OffsetDateTime::now_utc(),
        };

        info!("Running transaction");

        let mut tx: Transaction<'_, Postgres> = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        info!("Ran transaction");

        // Insert the team
        sqlx::query!(
            r#"
                INSERT INTO teams (id, name, created_at, created_by_user_id)
                VALUES ($1, $2, $3, $4)
            "#,
            team.id,
            team.name,
            team.created_at,
            team.created_by_user_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert user {:?}", err);
            DaoError::InternalServerError("Failed to insert user".to_string())
        })?;

        // Insert the membership
        sqlx::query!(
            r#"
                INSERT INTO team_members (team_id, user_id)
                VALUES ($1, $2)
            "#,
            team.id,
            team.created_by_user_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert membership {:?}", err);
            DaoError::InternalServerError("Failed to insert membership".to_string())
        })?;

        // Commit the transaction
        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to run transaction".to_string())
        })?;

        Ok(team)
    }

    pub async fn list_user_teams(&self, user_id: String) -> Result<Vec<Team>, DaoError> {
        info!("Listing user teams user_id={}", user_id);
        let teams = query_as!(
            Team,
            r#"
            SELECT t.id, t.name, t.created_at, t.created_by_user_id
            FROM teams t
            JOIN team_members m ON m.team_id = t.id
            WHERE m.user_id = $1
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list user teams {:?}", err);
            DaoError::InternalServerError("Failed to list user teams".to_string())
        })?;

        Ok(teams)
    }

    pub async fn get_user_team(
        &self,
        user_id: String,
        team_id: String,
    ) -> Result<Option<Team>, DaoError> {
        info!("Getting user team user_id={} team_id={}", user_id, team_id);
        let team = query_as!(
            Team,
            r#"
            SELECT t.id, t.name, t.created_at, t.created_by_user_id
            FROM teams t
            JOIN team_members m ON m.team_id = t.id
            WHERE m.user_id = $1 AND t.id = $2
            "#,
            user_id,
            team_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list user teams {:?}", err);
            DaoError::InternalServerError("Failed to list user teams".to_string())
        })?;

        Ok(team)
    }

    pub async fn add_user_to_team(
        &self,
        team_id: &String,
        user_id: &String,
    ) -> Result<(), DaoError> {
        info!(
            "Creating team membership team_id={} user_id={}",
            team_id, user_id
        );

        // TODO Share this code with create team
        sqlx::query!(
            r#"
                INSERT INTO team_members (team_id, user_id)
                VALUES ($1, $2)
            "#,
            team_id,
            user_id
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to insert membership {:?}", err);
            DaoError::InternalServerError("Failed to insert membership".to_string())
        })?;

        Ok(())
    }

    pub async fn list_team_members(&self, team_id: &String) -> Result<Vec<User>, DaoError> {
        info!("Fetching team members for team_id={}", team_id);

        let members = sqlx::query_as!(
            User,
            r#"
                SELECT u.id, u.first_name, u.last_name, u.email, u.created_at
                FROM users u
                JOIN team_members tm ON u.id = tm.user_id
                WHERE tm.team_id = $1
            "#,
            team_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!(
                "Failed to fetch team members for team_id={}: {:?}",
                team_id, err
            );
            DaoError::InternalServerError("Failed to fetch team members".to_string())
        })?;

        Ok(members)
    }
}
