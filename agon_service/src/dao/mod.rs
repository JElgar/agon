use base64::{Engine, prelude::BASE64_URL_SAFE};
use bigdecimal::BigDecimal;
use chrono::{NaiveDateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Transaction, Type, query, query_as};
use thiserror::Error;
use tracing::{error, info};

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
#[sqlx(type_name = "game_type", rename_all = "snake_case")]
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

pub struct Group {
    pub id: String,
    pub name: String,
    pub created_by_user_id: String,
    pub created_at: NaiveDateTime,
}

#[derive(Clone)]
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

pub struct GameTeam {
    pub id: String,
    pub game_id: String,
    pub name: String,
    pub color: Option<String>,
    // TODO Remove this?
    pub position: i32,
    pub created_at: NaiveDateTime,
}

pub struct GameInvitation {
    pub game_id: String,
    pub user_id: String,
    pub team_id: String,
    pub status: InvitationStatus,
    pub invited_at: NaiveDateTime,
    pub responded_at: Option<NaiveDateTime>,
}

pub struct CreateGameTeamInput {
    pub name: String,
    pub color: Option<String>,
    pub position: i32,
    pub invited_user_ids: Vec<String>,
}

pub struct CreateGameInput {
    pub created_by_user_id: String,
    pub title: String,
    pub game_type: GameType,
    pub location_latitude: BigDecimal,
    pub location_longitude: BigDecimal,
    pub location_name: Option<String>,
    pub scheduled_time: NaiveDateTime,
    pub duration_minutes: i32,
    pub teams: Vec<CreateGameTeamInput>,
}

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

    pub async fn create_group(&self, user_id: String, name: String) -> Result<Group, DaoError> {
        info!("Creating group");

        let group = Group {
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

        // Insert the group
        sqlx::query!(
            r#"
                INSERT INTO groups (id, name, created_at, created_by_user_id)
                VALUES ($1, $2, $3, $4)
            "#,
            group.id,
            group.name,
            group.created_at,
            group.created_by_user_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert group {:?}", err);
            DaoError::InternalServerError("Failed to insert group".to_string())
        })?;

        // Insert the membership
        sqlx::query!(
            r#"
                INSERT INTO group_members (group_id, user_id)
                VALUES ($1, $2)
            "#,
            group.id,
            group.created_by_user_id
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

        Ok(group)
    }

    pub async fn list_user_groups(&self, user_id: String) -> Result<Vec<Group>, DaoError> {
        info!("Listing user groups user_id={}", user_id);
        let groups = query_as!(
            Group,
            r#"
            SELECT g.id, g.name, g.created_at, g.created_by_user_id
            FROM groups g
            JOIN group_members m ON m.group_id = g.id
            WHERE m.user_id = $1
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list user groups {:?}", err);
            DaoError::InternalServerError("Failed to list user groups".to_string())
        })?;

        Ok(groups)
    }

    pub async fn get_user_group(
        &self,
        user_id: String,
        group_id: String,
    ) -> Result<Option<Group>, DaoError> {
        info!(
            "Getting user group user_id={} group_id={}",
            user_id, group_id
        );
        let group = query_as!(
            Group,
            r#"
            SELECT g.id, g.name, g.created_at, g.created_by_user_id
            FROM groups g
            JOIN group_members m ON m.group_id = g.id
            WHERE m.user_id = $1 AND g.id = $2
            "#,
            user_id,
            group_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list user groups {:?}", err);
            DaoError::InternalServerError("Failed to list user groups".to_string())
        })?;

        Ok(group)
    }

    pub async fn add_user_to_group(
        &self,
        group_id: &String,
        user_id: &String,
    ) -> Result<(), DaoError> {
        info!(
            "Creating group membership group_id={} user_id={}",
            group_id, user_id
        );

        // TODO Share this code with create group
        sqlx::query!(
            r#"
                INSERT INTO group_members (group_id, user_id)
                VALUES ($1, $2)
            "#,
            group_id,
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

    pub async fn list_group_members(&self, group_id: &String) -> Result<Vec<User>, DaoError> {
        info!("Fetching group members for group_id={}", group_id);

        let members = sqlx::query_as!(
            User,
            r#"
                SELECT u.id, u.first_name, u.last_name, u.email, u.username, u.created_at
                FROM users u
                JOIN group_members gm ON u.id = gm.user_id
                WHERE gm.group_id = $1
            "#,
            group_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!(
                "Failed to fetch group members for group_id={}: {:?}",
                group_id, err
            );
            DaoError::InternalServerError("Failed to fetch group members".to_string())
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

    pub async fn add_game_invitations(
        &self,
        game_id: &str,
        user_ids: &[String],
    ) -> Result<(), DaoError> {
        info!(
            "Adding invitations for game {} to users {:?}",
            game_id, user_ids
        );

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
        info!(
            "User {} responding {:?} to game {}",
            user_id, response, game_id
        );

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

    pub async fn get_game_with_invitations(
        &self,
        game_id: &str,
    ) -> Result<Option<(Game, Vec<(User, GameInvitation)>)>, DaoError> {
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
                       gi.game_id, gi.team_id, gi.status as "status: InvitationStatus", gi.invited_at, gi.responded_at
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
                        team_id: row.team_id,
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

    /// Create a game with teams and invitations in a single transaction
    pub async fn create_game(
        &self,
        input: CreateGameInput,
    ) -> Result<Game, DaoError> {
        let game_id = generate_id();
        info!("Creating game id={}", &game_id);

        // Start transaction
        let mut tx = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        // Create the game
        let game = Game {
            id: game_id,
            title: input.title,
            game_type: input.game_type.clone(),
            location_latitude: input.location_latitude,
            location_longitude: input.location_longitude,
            location_name: input.location_name,
            scheduled_time: input.scheduled_time,
            duration_minutes: input.duration_minutes,
            created_by_user_id: input.created_by_user_id,
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
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert game {:?}", err);
            DaoError::InternalServerError("Failed to insert game".to_string())
        })?;

        // Create teams and invitations
        for team_input in input.teams {
            // Create team
            let team = GameTeam {
                id: generate_id(),
                game_id: game.id.clone(),
                name: team_input.name.clone(),
                color: team_input.color.clone(),
                position: team_input.position,
                created_at: Utc::now().naive_utc(),
            };

            query!(
                "INSERT INTO game_teams (id, game_id, name, color, position, created_at)
                VALUES ($1, $2, $3, $4, $5, $6)",
                team.id,
                team.game_id,
                team.name,
                team.color,
                team.position,
                team.created_at
            )
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert game team {:?}", err);
                DaoError::InternalServerError("Failed to insert game team".to_string())
            })?;

            // Create invitations for this team
            for user_id in team_input.invited_user_ids {
                query!(
                    "INSERT INTO game_invitations (game_id, user_id, team_id, status, invited_at)
                    VALUES ($1, $2, $3, $4, $5)
                    ON CONFLICT (game_id, user_id) DO UPDATE SET team_id = $3",
                    game.id,
                    user_id,
                    team.id,
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
        }

        // Commit the transaction
        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to commit transaction".to_string())
        })?;

        info!("Successfully created game with teams and invitations");
        Ok(game)
    }

    pub async fn list_game_teams(&self, game_id: &str) -> Result<Vec<GameTeam>, DaoError> {
        let teams = query_as!(
            GameTeam,
            r#"
            SELECT id, game_id, name, color, position, created_at
            FROM game_teams
            WHERE game_id = $1
            ORDER BY position
            "#,
            game_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list game teams {:?}", err);
            DaoError::InternalServerError("Failed to list game teams".to_string())
        })?;

        Ok(teams)
    }
}
