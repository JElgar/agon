use base64::{Engine, prelude::BASE64_URL_SAFE};
use bigdecimal::BigDecimal;
use chrono::{NaiveDateTime, NaiveDate, Utc, Duration, TimeZone};
use cron::Schedule;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, Transaction, Type, query, query_as};
use std::str::FromStr;
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
    #[sqlx(rename = "football_5_a_side")]
    Football5ASide,
    #[sqlx(rename = "football_11_a_side")]
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

// DAO-level schedule enum
#[derive(Debug, Clone)]
pub enum GameSchedule {
    OneOff {
        scheduled_time: NaiveDateTime,
    },
    Recurring {
        cron_schedule: String,
        start_date: NaiveDate,
        end_date: Option<NaiveDate>,
        occurrence_date: NaiveDate,
    },
}

// Updated Game struct with schedule
pub struct Game {
    pub id: String,
    pub title: String,
    pub game_type: GameType,
    pub location_latitude: BigDecimal,
    pub location_longitude: BigDecimal,
    pub location_name: Option<String>,
    pub duration_minutes: i32,
    pub created_by_user_id: String,
    pub created_at: NaiveDateTime,
    pub status: GameStatus,
    pub schedule: GameSchedule,
}

// Internal template struct
struct GameTemplate {
    pub id: String,
    pub title: String,
    pub game_type: GameType,
    pub location_latitude: BigDecimal,
    pub location_longitude: BigDecimal,
    pub location_name: Option<String>,
    pub duration_minutes: i32,
    pub created_by_user_id: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// Internal recurring game struct
struct RecurringGame {
    pub id: String,
    pub template_id: String,
    pub cron_schedule: String,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub last_generated_date: Option<NaiveDate>,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
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
    pub group_id: Option<String>,
    pub status: InvitationStatus,
    pub invited_at: NaiveDateTime,
    pub responded_at: Option<NaiveDateTime>,
}

pub struct GroupGameInvitation {
    pub game_id: String,
    pub group_id: String,
    pub invited_at: NaiveDateTime,
}

pub struct CreateGameTeamInput {
    pub name: String,
    pub color: Option<String>,
    pub position: i32,
    pub invited_user_ids: Vec<String>,
    pub invited_group_ids: Vec<String>,
}

// Schedule input types
#[derive(Debug, Clone)]
pub enum CreateGameSchedule {
    OneOff {
        scheduled_time: NaiveDateTime,
    },
    Recurring {
        cron_schedule: String,
        start_date: NaiveDate,
        end_date: Option<NaiveDate>,
    },
}

pub struct CreateGameInput {
    pub created_by_user_id: String,
    pub title: String,
    pub game_type: GameType,
    pub location_latitude: BigDecimal,
    pub location_longitude: BigDecimal,
    pub location_name: Option<String>,
    pub duration_minutes: i32,
    pub teams: Vec<CreateGameTeamInput>,
    pub schedule: CreateGameSchedule,
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
        team_id: &str,
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
                "INSERT INTO game_invitations (game_id, user_id, team_id, group_id, status, invited_at)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (game_id, user_id) DO NOTHING",
                game_id,
                user_id,
                team_id,
                None::<String>, // Additional invitations are not from a group
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

    pub async fn get_game(&self, game_id: &str) -> Result<Option<Game>, DaoError> {
        info!("Getting game with id={}", game_id);

        let result = query!(
            r#"
            SELECT 
                g.id, g.scheduled_time, g.occurrence_date,
                g.status as "status: GameStatus", g.created_at,
                t.title, t.game_type as "game_type: GameType", 
                t.location_latitude, t.location_longitude, t.location_name, 
                t.duration_minutes, t.created_by_user_id,
                -- Recurring game info (NULL for one-off games)
                rg.cron_schedule as "cron_schedule?", rg.start_date as "start_date?", rg.end_date as "end_date?"
            FROM games g
            JOIN game_templates t ON g.template_id = t.id
            LEFT JOIN recurring_games rg ON g.recurring_game_id = rg.id
            WHERE g.id = $1
            "#,
            game_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to get game {:?}", err);
            DaoError::InternalServerError("Failed to get game".to_string())
        })?;

        Ok(result.map(|row| {
            let schedule = if let Some(cron_schedule) = row.cron_schedule {
                // This is a recurring game
                GameSchedule::Recurring {
                    cron_schedule,
                    start_date: row.start_date.unwrap(),
                    end_date: row.end_date,
                    occurrence_date: row.occurrence_date.unwrap(),
                }
            } else {
                // This is a one-off game
                GameSchedule::OneOff {
                    scheduled_time: row.scheduled_time,
                }
            };

            Game {
                id: row.id,
                title: row.title,
                game_type: row.game_type,
                location_latitude: row.location_latitude,
                location_longitude: row.location_longitude,
                location_name: row.location_name,
                duration_minutes: row.duration_minutes,
                created_by_user_id: row.created_by_user_id,
                created_at: row.created_at,
                status: row.status,
                schedule,
            }
        }))
    }

    pub async fn list_user_games(&self, user_id: &str) -> Result<Vec<Game>, DaoError> {
        info!("Listing games for user {}", user_id);

        let results = query!(
            r#"
            SELECT DISTINCT 
                g.id, g.scheduled_time, g.occurrence_date,
                g.status as "status: GameStatus", g.created_at,
                t.title, t.game_type as "game_type: GameType", 
                t.location_latitude, t.location_longitude, t.location_name, 
                t.duration_minutes, t.created_by_user_id,
                -- Recurring game info (NULL for one-off games)
                rg.cron_schedule as "cron_schedule?", rg.start_date as "start_date?", rg.end_date as "end_date?"
            FROM games g
            JOIN game_templates t ON g.template_id = t.id
            LEFT JOIN recurring_games rg ON g.recurring_game_id = rg.id
            LEFT JOIN game_invitations gi ON g.id = gi.game_id
            WHERE t.created_by_user_id = $1 OR gi.user_id = $1
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

        Ok(results.into_iter().map(|row| {
            let schedule = if let Some(cron_schedule) = row.cron_schedule {
                // This is a recurring game
                GameSchedule::Recurring {
                    cron_schedule,
                    start_date: row.start_date.unwrap(),
                    end_date: row.end_date,
                    occurrence_date: row.occurrence_date.unwrap(),
                }
            } else {
                // This is a one-off game
                GameSchedule::OneOff {
                    scheduled_time: row.scheduled_time,
                }
            };

            Game {
                id: row.id,
                title: row.title,
                game_type: row.game_type,
                location_latitude: row.location_latitude,
                location_longitude: row.location_longitude,
                location_name: row.location_name,
                duration_minutes: row.duration_minutes,
                created_by_user_id: row.created_by_user_id,
                created_at: row.created_at,
                status: row.status,
                schedule,
            }
        }).collect())
    }

    pub async fn get_game_with_invitations(
        &self,
        game_id: &str,
    ) -> Result<Option<(Game, Vec<(User, GameInvitation)>)>, DaoError> {
        info!("Getting game with invitations for game {}", game_id);

        let game = self.get_game(game_id).await?;

        if let Some(game) = game {
            let invitations = sqlx::query!(
                r#"
                SELECT u.id as user_id, u.first_name, u.last_name, u.email, u.username, u.created_at as user_created_at,
                       gi.game_id, gi.team_id, gi.group_id, gi.status as "status: InvitationStatus", gi.invited_at, gi.responded_at
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
                        group_id: row.group_id,
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
        match &input.schedule {
            CreateGameSchedule::OneOff { scheduled_time } => {
                let template = self.create_game_template(&input).await?;
                self.create_game_from_template(&template.id, *scheduled_time, None, None).await
            }
            CreateGameSchedule::Recurring { cron_schedule, start_date, end_date } => {
                let template = self.create_game_template(&input).await?;
                let recurring_game = self.create_recurring_game(&template.id, cron_schedule, *start_date, *end_date).await?;
                
                // Generate initial games and return the first one
                self.generate_games_for_recurring_game(&recurring_game).await?;
                let first_game = self.get_first_generated_game(&recurring_game.id).await?;
                first_game.ok_or_else(|| DaoError::InternalServerError("Failed to generate first game".to_string()))
            }
        }
    }

    /// Internal helper: Create game template
    async fn create_game_template(&self, input: &CreateGameInput) -> Result<GameTemplate, DaoError> {
        let template_id = generate_id();
        let now = Utc::now().naive_utc();
        
        let mut tx = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        // Create the template
        let template = GameTemplate {
            id: template_id.clone(),
            title: input.title.clone(),
            game_type: input.game_type.clone(),
            location_latitude: input.location_latitude.clone(),
            location_longitude: input.location_longitude.clone(),
            location_name: input.location_name.clone(),
            duration_minutes: input.duration_minutes,
            created_by_user_id: input.created_by_user_id.clone(),
            created_at: now,
            updated_at: now,
        };

        query!(
            "INSERT INTO game_templates (id, title, game_type, location_latitude, location_longitude, location_name, duration_minutes, created_by_user_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            template.id,
            template.title,
            template.game_type.clone() as GameType,
            template.location_latitude,
            template.location_longitude,
            template.location_name,
            template.duration_minutes,
            template.created_by_user_id,
            template.created_at,
            template.updated_at
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert game template {:?}", err);
            DaoError::InternalServerError("Failed to insert game template".to_string())
        })?;

        // Create template teams and invitations
        for team_input in &input.teams {
            // Create template team
            let team_id = generate_id();
            
            query!(
                "INSERT INTO game_template_teams (id, template_id, name, color, position, created_at)
                VALUES ($1, $2, $3, $4, $5, $6)",
                team_id,
                template.id,
                team_input.name,
                team_input.color,
                team_input.position,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert game template team {:?}", err);
                DaoError::InternalServerError("Failed to insert game template team".to_string())
            })?;

            // Create template invitations for individual users in this team
            for user_id in &team_input.invited_user_ids {
                query!(
                    "INSERT INTO game_template_invitations (id, template_id, user_id, group_id, team_id, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6)",
                    generate_id(),
                    template.id,
                    user_id,
                    None::<String>, // Direct user invitation, not from a group
                    team_id,
                    now
                )
                .execute(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to insert game template invitation {:?}", err);
                    DaoError::InternalServerError("Failed to insert game template invitation".to_string())
                })?;
            }

            // Create template group invitations for this team
            for group_id in &team_input.invited_group_ids {
                query!(
                    "INSERT INTO game_template_invitations (id, template_id, user_id, group_id, team_id, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6)",
                    generate_id(),
                    template.id,
                    None::<String>, // No specific user, this is a group invitation
                    Some(group_id.clone()),
                    team_id,
                    now
                )
                .execute(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to insert game template group invitation {:?}", err);
                    DaoError::InternalServerError("Failed to insert game template group invitation".to_string())
                })?;
            }
        }

        // Commit the transaction
        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to commit transaction".to_string())
        })?;

        Ok(template)
    }

    /// Internal helper: Create recurring game
    async fn create_recurring_game(
        &self,
        template_id: &str,
        cron_schedule: &str,
        start_date: NaiveDate,
        end_date: Option<NaiveDate>,
    ) -> Result<RecurringGame, DaoError> {
        let recurring_id = generate_id();
        let now = Utc::now().naive_utc();

        let recurring_game = RecurringGame {
            id: recurring_id.clone(),
            template_id: template_id.to_string(),
            cron_schedule: cron_schedule.to_string(),
            start_date,
            end_date,
            last_generated_date: None,
            is_active: true,
            created_at: now,
        };

        query!(
            "INSERT INTO recurring_games (id, template_id, cron_schedule, start_date, end_date, last_generated_date, is_active, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            recurring_game.id,
            recurring_game.template_id,
            recurring_game.cron_schedule,
            recurring_game.start_date,
            recurring_game.end_date,
            recurring_game.last_generated_date,
            recurring_game.is_active,
            recurring_game.created_at
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to insert recurring game {:?}", err);
            DaoError::InternalServerError("Failed to insert recurring game".to_string())
        })?;

        Ok(recurring_game)
    }

    /// Internal helper: Create game instance from template
    async fn create_game_from_template(
        &self,
        template_id: &str,
        scheduled_time: NaiveDateTime,
        recurring_game_id: Option<String>,
        occurrence_date: Option<NaiveDate>,
    ) -> Result<Game, DaoError> {
        let game_id = generate_id();
        let now = Utc::now().naive_utc();

        let mut tx = self.pool.begin().await.map_err(|err| {
            error!("Failed to start transaction {:?}", err);
            DaoError::InternalServerError("Failed to start transaction".to_string())
        })?;

        // Insert game instance
        query!(
            "INSERT INTO games (id, template_id, recurring_game_id, scheduled_time, occurrence_date, status, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)",
            game_id,
            template_id,
            recurring_game_id,
            scheduled_time,
            occurrence_date,
            GameStatus::Scheduled as GameStatus,
            now
        )
        .execute(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to insert game {:?}", err);
            DaoError::InternalServerError("Failed to insert game".to_string())
        })?;

        // Copy template teams to game teams
        let template_teams = query!(
            "SELECT id, name, color, position FROM game_template_teams WHERE template_id = $1 ORDER BY position",
            template_id
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to fetch template teams {:?}", err);
            DaoError::InternalServerError("Failed to fetch template teams".to_string())
        })?;

        let mut team_id_mapping = std::collections::HashMap::new();

        for template_team in template_teams {
            let team_id = generate_id();
            team_id_mapping.insert(template_team.id.clone(), team_id.clone());

            query!(
                "INSERT INTO game_teams (id, game_id, template_team_id, name, color, position, created_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)",
                team_id,
                game_id,
                Some(template_team.id),
                template_team.name,
                template_team.color,
                template_team.position,
                now
            )
            .execute(&mut *tx)
            .await
            .map_err(|err| {
                error!("Failed to insert game team {:?}", err);
                DaoError::InternalServerError("Failed to insert game team".to_string())
            })?;
        }

        // Copy template invitations to game invitations
        let template_invitations = query!(
            "SELECT user_id, group_id, team_id FROM game_template_invitations WHERE template_id = $1",
            template_id
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|err| {
            error!("Failed to fetch template invitations {:?}", err);
            DaoError::InternalServerError("Failed to fetch template invitations".to_string())
        })?;

        for template_invitation in template_invitations {
            let game_team_id = team_id_mapping.get(&template_invitation.team_id)
                .ok_or_else(|| DaoError::InternalServerError("Team mapping not found".to_string()))?;

            if let Some(user_id) = template_invitation.user_id {
                // Direct user invitation
                query!(
                    "INSERT INTO game_invitations (game_id, user_id, team_id, group_id, status, invited_at)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    ON CONFLICT (game_id, user_id) DO UPDATE SET team_id = $3, group_id = $4",
                    game_id,
                    user_id,
                    game_team_id,
                    None::<String>,
                    InvitationStatus::Pending as InvitationStatus,
                    now
                )
                .execute(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to insert game invitation {:?}", err);
                    DaoError::InternalServerError("Failed to insert game invitation".to_string())
                })?;
            } else if let Some(group_id) = template_invitation.group_id {
                // Group invitation - expand to individual users
                query!(
                    "INSERT INTO group_game_invitations (game_id, group_id, invited_at)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (game_id, group_id) DO NOTHING",
                    game_id,
                    group_id,
                    now
                )
                .execute(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to insert group game invitation {:?}", err);
                    DaoError::InternalServerError("Failed to insert group game invitation".to_string())
                })?;

                // Get group members and create individual invitations
                let group_members = query!(
                    "SELECT user_id FROM group_members WHERE group_id = $1",
                    group_id
                )
                .fetch_all(&mut *tx)
                .await
                .map_err(|err| {
                    error!("Failed to fetch group members {:?}", err);
                    DaoError::InternalServerError("Failed to fetch group members".to_string())
                })?;

                for member in group_members {
                    query!(
                        "INSERT INTO game_invitations (game_id, user_id, team_id, group_id, status, invited_at)
                        VALUES ($1, $2, $3, $4, $5, $6)
                        ON CONFLICT (game_id, user_id) DO UPDATE SET team_id = $3, group_id = $4",
                        game_id,
                        member.user_id,
                        game_team_id,
                        Some(group_id.clone()),
                        InvitationStatus::Pending as InvitationStatus,
                        now
                    )
                    .execute(&mut *tx)
                    .await
                    .map_err(|err| {
                        error!("Failed to insert group member game invitation {:?}", err);
                        DaoError::InternalServerError("Failed to insert group member game invitation".to_string())
                    })?;
                }
            }
        }

        tx.commit().await.map_err(|err| {
            error!("Failed to commit transaction {:?}", err);
            DaoError::InternalServerError("Failed to commit transaction".to_string())
        })?;

        // Fetch the created game with template data
        self.get_game(&game_id).await?
            .ok_or_else(|| DaoError::InternalServerError("Failed to fetch created game".to_string()))
    }

    /// Generate games for recurring game using cron schedule
    async fn generate_games_for_recurring_game(&self, recurring_game: &RecurringGame) -> Result<(), DaoError> {
        const GENERATE_AHEAD_DAYS: i64 = 30;
        
        info!("Generating games for recurring game {}", recurring_game.id);

        // Parse the cron schedule
        let schedule = Schedule::from_str(&recurring_game.cron_schedule)
            .map_err(|err| {
                error!("Invalid cron schedule {}: {:?}", recurring_game.cron_schedule, err);
                DaoError::InternalServerError("Invalid cron schedule".to_string())
            })?;

        // Calculate the date range for generation
        let start_date = recurring_game.last_generated_date
            .unwrap_or(recurring_game.start_date);
        let end_date = std::cmp::min(
            recurring_game.end_date.unwrap_or(NaiveDate::MAX),
            Utc::now().date_naive() + Duration::days(GENERATE_AHEAD_DAYS)
        );

        info!("Generating games from {} to {}", start_date, end_date);

        // Get upcoming occurrences
        let start_datetime = Utc.from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap());
        let mut generated_count = 0;
        let mut last_generated_date = start_date;

        for datetime in schedule.after(&start_datetime) {
            let occurrence_date = datetime.date_naive();
            
            // Stop if we've reached the end date
            if occurrence_date > end_date {
                break;
            }

            // Skip if we already generated this date
            if occurrence_date <= start_date {
                continue;
            }

            // Check if a game already exists for this occurrence
            let existing_game = query!(
                "SELECT id FROM games WHERE recurring_game_id = $1 AND occurrence_date = $2",
                recurring_game.id,
                occurrence_date
            )
            .fetch_optional(&self.pool)
            .await
            .map_err(|err| {
                error!("Failed to check existing game {:?}", err);
                DaoError::InternalServerError("Failed to check existing game".to_string())
            })?;

            if existing_game.is_some() {
                info!("Game already exists for {}, skipping", occurrence_date);
                last_generated_date = occurrence_date;
                continue;
            }

            // Generate game for this occurrence
            let scheduled_time = datetime.naive_utc();
            let _game = self.create_game_from_template(
                &recurring_game.template_id,
                scheduled_time,
                Some(recurring_game.id.clone()),
                Some(occurrence_date),
            ).await?;

            generated_count += 1;
            last_generated_date = occurrence_date;
            info!("Generated game for {}", occurrence_date);

            // Limit the number of games generated in one batch
            if generated_count >= 10 {
                break;
            }
        }

        // Update the last_generated_date
        if generated_count > 0 {
            query!(
                "UPDATE recurring_games SET last_generated_date = $1 WHERE id = $2",
                last_generated_date,
                recurring_game.id
            )
            .execute(&self.pool)
            .await
            .map_err(|err| {
                error!("Failed to update last_generated_date {:?}", err);
                DaoError::InternalServerError("Failed to update last_generated_date".to_string())
            })?;
        }

        info!("Generated {} games for recurring game {}", generated_count, recurring_game.id);
        Ok(())
    }

    /// Get first generated game for recurring series
    async fn get_first_generated_game(&self, recurring_game_id: &str) -> Result<Option<Game>, DaoError> {
        info!("Getting first generated game for recurring game {}", recurring_game_id);

        let result = query!(
            "SELECT id FROM games WHERE recurring_game_id = $1 ORDER BY occurrence_date ASC LIMIT 1",
            recurring_game_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to get first generated game {:?}", err);
            DaoError::InternalServerError("Failed to get first generated game".to_string())
        })?;

        if let Some(row) = result {
            self.get_game(&row.id).await
        } else {
            Ok(None)
        }
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

    /// Get games where a specific group was invited
    pub async fn list_group_games(&self, group_id: &str) -> Result<Vec<Game>, DaoError> {
        info!("Listing games for group {}", group_id);

        let results = query!(
            r#"
            SELECT DISTINCT 
                g.id, g.scheduled_time, g.occurrence_date,
                g.status as "status: GameStatus", g.created_at,
                t.title, t.game_type as "game_type: GameType", 
                t.location_latitude, t.location_longitude, t.location_name, 
                t.duration_minutes, t.created_by_user_id,
                -- Recurring game info (NULL for one-off games)
                rg.cron_schedule as "cron_schedule?", rg.start_date as "start_date?", rg.end_date as "end_date?"
            FROM games g
            JOIN game_templates t ON g.template_id = t.id
            LEFT JOIN recurring_games rg ON g.recurring_game_id = rg.id
            JOIN group_game_invitations ggi ON g.id = ggi.game_id
            WHERE ggi.group_id = $1
            ORDER BY g.scheduled_time DESC
            "#,
            group_id
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to list group games {:?}", err);
            DaoError::InternalServerError("Failed to list group games".to_string())
        })?;

        Ok(results.into_iter().map(|row| {
            let schedule = if let Some(cron_schedule) = row.cron_schedule {
                // This is a recurring game
                GameSchedule::Recurring {
                    cron_schedule,
                    start_date: row.start_date.unwrap(),
                    end_date: row.end_date,
                    occurrence_date: row.occurrence_date.unwrap(),
                }
            } else {
                // This is a one-off game
                GameSchedule::OneOff {
                    scheduled_time: row.scheduled_time,
                }
            };

            Game {
                id: row.id,
                title: row.title,
                game_type: row.game_type,
                location_latitude: row.location_latitude,
                location_longitude: row.location_longitude,
                location_name: row.location_name,
                duration_minutes: row.duration_minutes,
                created_by_user_id: row.created_by_user_id,
                created_at: row.created_at,
                status: row.status,
                schedule,
            }
        }).collect())
    }

    /// Add group invitation to a game
    pub async fn add_group_game_invitation(
        &self,
        game_id: &str,
        group_id: &str,
    ) -> Result<(), DaoError> {
        info!("Adding group {} invitation to game {}", group_id, game_id);

        query!(
            "INSERT INTO group_game_invitations (game_id, group_id, invited_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (game_id, group_id) DO NOTHING",
            game_id,
            group_id,
            Utc::now().naive_utc(),
        )
        .execute(&self.pool)
        .await
        .map_err(|err| {
            error!("Failed to insert group game invitation {:?}", err);
            DaoError::InternalServerError("Failed to insert group game invitation".to_string())
        })?;

        Ok(())
    }
}
