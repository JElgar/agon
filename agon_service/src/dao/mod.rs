use base64::{prelude::BASE64_URL_SAFE, Engine};
use bigdecimal::BigDecimal;
use rand::Rng;
use sqlx::{Pool, Postgres, Transaction, query, query_as, Type};
use thiserror::Error;
use tracing::{error, info};
use serde::{Deserialize, Serialize};
use chrono::{NaiveDateTime, Utc};

fn generate_id() -> String {
    let random_bytes: [u8; 8] = rand::rng().random();
    BASE64_URL_SAFE.encode(random_bytes)
}

#[derive(Error, Debug)]
pub enum DaoError {
    #[error("internal error")]
    InternalServerError(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[sqlx(type_name = "game_status", rename_all = "snake_case")]
pub enum GameStatus {
    Scheduled,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[sqlx(type_name = "invitation_status", rename_all = "snake_case")]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[sqlx(type_name = "game_type", rename_all="snake_case")]
pub enum GameType {
    Football5ASide,
    Football11ASide,
    Basketball,
    Tennis,
    Badminton,
    Cricket,
    Rugby,
    Hockey,
    Other,
}

pub struct Team {
    pub id: String,
    pub name: String,
    pub created_by_user_id: String,
    pub created_at: NaiveDateTime,
}

pub struct User {
    pub id: String,
    pub email: String,
    pub first_name: String,
    pub last_name: String,
    pub username: String,
    pub created_at: NaiveDateTime,
}

pub struct Game {
    pub id: String,
    pub title: String,
    pub game_type: GameType,
    pub location_latitude: BigDecimal,
    pub location_longitude: BigDecimal,
    pub location_name: Option<String>,
    pub scheduled_time: NaiveDateTime,
    pub duration_minutes: i32,
    pub created_by_user_id: String,
    pub created_at: NaiveDateTime,
    pub status: GameStatus,
}

pub struct GameInvitation {
    pub game_id: String,
    pub user_id: String,
    pub status: InvitationStatus,
    pub invited_at: NaiveDateTime,
    pub responded_at: Option<NaiveDateTime>,
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

    pub async fn get_user(&self, user_id: &str) -> Result<Option<User>, DaoError> {
        info!("Getting user with id={}", user_id);

        let user = query_as!(
            User,
            r#"
            SELECT id, first_name, last_name, email, username, created_at
            FROM users
            WHERE id = $1
            "#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to get user {:?}", err);
            DaoError::InternalServerError("Failed to get user".to_string())
        })?;

        Ok(user)
    }

    pub async fn create_user(
        &self,
        sub: String,
        email: String,
        first_name: String,
        last_name: String,
        username: String,
    ) -> Result<User, DaoError> {
        let user = User {
            id: sub,
            email,
            first_name,
            last_name,
            username: username.clone(),
            created_at: Utc::now().naive_utc(),
        };

        query!(
            "INSERT INTO users (id, first_name, last_name, email, username, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)",
            user.id,
            user.first_name,
            user.last_name,
            user.email,
            username,
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
            created_at: Utc::now().naive_utc(),
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
            error!("Failed to insert team {:?}", err);
            DaoError::InternalServerError("Failed to insert team".to_string())
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
                SELECT u.id, u.first_name, u.last_name, u.email, u.username, u.created_at
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

    pub async fn search_users(&self, query: &str) -> Result<Vec<User>, DaoError> {
        info!("Searching users with query={}", query);

        let users = sqlx::query_as!(
            User,
            r#"
                SELECT id, first_name, last_name, email, username, created_at
                FROM users
                WHERE username ILIKE $1 
                   OR first_name ILIKE $1 
                   OR last_name ILIKE $1
                   OR CONCAT(first_name, ' ', last_name) ILIKE $1
                ORDER BY username
                LIMIT 20
            "#,
            format!("%{}%", query)
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to search users: {:?}", err);
            DaoError::InternalServerError("Failed to search users".to_string())
        })?;

        Ok(users)
    }

    pub async fn create_game(
        &self,
        created_by_user_id: String,
        title: String,
        game_type: GameType,
        location_latitude: BigDecimal,
        location_longitude: BigDecimal,
        location_name: Option<String>,
        scheduled_time: NaiveDateTime,
        duration_minutes: i32,
    ) -> Result<Game, DaoError> {
        let game = Game {
            id: generate_id(),
            title,
            game_type: game_type.clone(),
            location_latitude,
            location_longitude,
            location_name: location_name.clone(),
            scheduled_time,
            duration_minutes,
            created_by_user_id,
            created_at: Utc::now().naive_utc(),
            status: GameStatus::Scheduled,
        };

        query!(
            "INSERT INTO games (id, title, game_type, location_latitude, location_longitude, location_name, scheduled_time, duration_minutes, created_by_user_id, created_at, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
            game.id,
            game.title,
            game.game_type.clone() as GameType,
            game.location_latitude,
            game.location_longitude,
            game.location_name,
            game.scheduled_time,
            game.duration_minutes,
            game.created_by_user_id,
            game.created_at,
            game.status.clone() as GameStatus
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to insert game {:?}", err);
            DaoError::InternalServerError("Failed to insert game".to_string())
        })?;

        Ok(game)
    }

    pub async fn invite_users_to_game(
        &self,
        game_id: &str,
        user_ids: &[String],
    ) -> Result<(), DaoError> {
        info!("Inviting {} users to game {}", user_ids.len(), game_id);

        let mut tx = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        for user_id in user_ids {
            query!(
                "INSERT INTO game_invitations (game_id, user_id, status, invited_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (game_id, user_id) DO NOTHING",
                game_id,
                user_id,
                InvitationStatus::Pending as InvitationStatus,
                Utc::now().naive_utc(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert game invitation {:?}", err);
                DaoError::InternalServerError("Failed to insert game invitation".to_string())
            })?;
        }

        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to run transaction".to_string())
        })?;

        Ok(())
    }

    pub async fn add_game_invitations(&self, game_id: &str, user_ids: &[String]) -> Result<(), DaoError> {
        info!("Adding invitations for game {} to users {:?}", game_id, user_ids);

        let mut tx = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        for user_id in user_ids {
            query!(
                "INSERT INTO game_invitations (game_id, user_id, status, invited_at)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (game_id, user_id) DO NOTHING",
                game_id,
                user_id,
                InvitationStatus::Pending as InvitationStatus,
                Utc::now().naive_utc(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert game invitation {:?}", err);
                DaoError::InternalServerError("Failed to insert game invitation".to_string())
            })?;
        }

        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to run transaction".to_string())
        })?;

        Ok(())
    }

    pub async fn respond_to_game_invitation(
        &self,
        game_id: &str,
        user_id: &str,
        response: InvitationStatus,
    ) -> Result<(), DaoError> {
        info!("User {} responding {:?} to game {}", user_id, response, game_id);

        query!(
            "UPDATE game_invitations 
            SET status = $1, responded_at = $2 
            WHERE game_id = $3 AND user_id = $4",
            response as InvitationStatus,
            Utc::now().naive_utc(),
            game_id,
            user_id
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to update game invitation {:?}", err);
            DaoError::InternalServerError("Failed to update game invitation".to_string())
        })?;

        Ok(())
    }

    pub async fn list_user_games(&self, user_id: &str) -> Result<Vec<Game>, DaoError> {
        info!("Listing games for user {}", user_id);

        let games = query_as!(
            Game,
            r#"
            SELECT DISTINCT g.id, g.title, g.game_type as "game_type: GameType", 
                   g.location_latitude, g.location_longitude, g.location_name,
                   g.scheduled_time, g.duration_minutes, g.created_by_user_id, 
                   g.created_at, g.status as "status: GameStatus"
            FROM games g
            LEFT JOIN game_invitations gi ON g.id = gi.game_id
            WHERE g.created_by_user_id = $1 OR gi.user_id = $1
            ORDER BY g.scheduled_time DESC
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list user games {:?}", err);
            DaoError::InternalServerError("Failed to list user games".to_string())
        })?;

        Ok(games)
    }

    pub async fn get_game_with_invitations(&self, game_id: &str) -> Result<Option<(Game, Vec<(User, GameInvitation)>)>, DaoError> {
        info!("Getting game with invitations for game {}", game_id);

        let game = query_as!(
            Game,
            r#"
            SELECT id, title, game_type as "game_type: GameType", 
                   location_latitude, location_longitude, location_name,
                   scheduled_time, duration_minutes, created_by_user_id, 
                   created_at, status as "status: GameStatus"
            FROM games
            WHERE id = $1
            "#,
            game_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to get game {:?}", err);
            DaoError::InternalServerError("Failed to get game".to_string())
        })?;

        if let Some(game) = game {
            let invitations = sqlx::query!(
                r#"
                SELECT u.id as user_id, u.first_name, u.last_name, u.email, u.username, u.created_at as user_created_at,
                       gi.game_id, gi.status as "status: InvitationStatus", gi.invited_at, gi.responded_at
                FROM game_invitations gi
                JOIN users u ON gi.user_id = u.id
                WHERE gi.game_id = $1
                ORDER BY gi.invited_at
                "#,
                game_id
            )
            .fetch_all(&self.pool)
            .await
            .map_err(|err| {
                error!("Failed to get game invitations {:?}", err);
                DaoError::InternalServerError("Failed to get game invitations".to_string())
            })?;

            let user_invitations: Vec<(User, GameInvitation)> = invitations
                .into_iter()
                .map(|row| {
                    let user = User {
                        id: row.user_id,
                        first_name: row.first_name,
                        last_name: row.last_name,
                        email: row.email,
                        username: row.username,
                        created_at: row.user_created_at,
                    };
                    let invitation = GameInvitation {
                        game_id: row.game_id,
                        user_id: user.id.clone(),
                        status: row.status, // sqlx will handle the enum conversion
                        invited_at: row.invited_at,
                        responded_at: row.responded_at,
                    };
                    (user, invitation)
                })
                .collect();

            Ok(Some((game, user_invitations)))
        } else {
            Ok(None)
        }
    }
}
