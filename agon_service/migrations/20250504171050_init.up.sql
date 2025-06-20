-- Add up migration script here

-- Create enums for status fields
CREATE TYPE invitation_status AS ENUM ('pending', 'accepted', 'declined');
CREATE TYPE game_status AS ENUM ('scheduled', 'in_progress', 'completed', 'cancelled');
CREATE TYPE game_type AS ENUM ('football_5_a_side', 'football_11_a_side', 'basketball', 'tennis', 'badminton', 'cricket', 'rugby', 'hockey', 'other');

-- Users table
CREATE TABLE users (
	id TEXT PRIMARY KEY,
	username TEXT NOT NULL UNIQUE,
	first_name TEXT NOT NULL,
	last_name TEXT NOT NULL,
	email TEXT NOT NULL,
	created_at TIMESTAMP NOT NULL
);

-- Groups table
CREATE TABLE groups (
	id VARCHAR(12) PRIMARY KEY,
	name TEXT NOT NULL,
	created_by_user_id TEXT REFERENCES users(id) NOT NULL,
	created_at TIMESTAMP NOT NULL
);

-- Group members junction table
CREATE TABLE group_members (
	user_id TEXT REFERENCES users(id),
	group_id VARCHAR(12) REFERENCES groups(id),
	PRIMARY KEY (user_id, group_id)
);

-- Game templates table (holds all game metadata)
CREATE TABLE game_templates (
    id VARCHAR(12) PRIMARY KEY,
    title TEXT NOT NULL,
    game_type game_type NOT NULL,
    location_latitude DECIMAL(10, 8) NOT NULL,
    location_longitude DECIMAL(11, 8) NOT NULL,
    location_name TEXT,
    duration_minutes INTEGER NOT NULL,
    created_by_user_id TEXT NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL
);

-- Recurring games table
CREATE TABLE recurring_games (
    id VARCHAR(12) PRIMARY KEY,
    template_id VARCHAR(12) NOT NULL REFERENCES game_templates(id),
    cron_schedule TEXT NOT NULL,
    start_date DATE NOT NULL,
    end_date DATE,
    last_generated_date DATE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL
);

-- Games table (leaner - just instance data)
CREATE TABLE games (
    id VARCHAR(12) PRIMARY KEY,
    template_id VARCHAR(12) NOT NULL REFERENCES game_templates(id),
    recurring_game_id VARCHAR(12) REFERENCES recurring_games(id),
    scheduled_time TIMESTAMP NOT NULL,
    occurrence_date DATE, -- For recurring games, the date this represents
    status game_status NOT NULL DEFAULT 'scheduled',
    created_at TIMESTAMP NOT NULL
);

-- Template teams table
CREATE TABLE game_template_teams (
    id VARCHAR(12) PRIMARY KEY,
    template_id VARCHAR(12) NOT NULL REFERENCES game_templates(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT,
    position INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL
);

-- Game teams table (instances)
CREATE TABLE game_teams (
    id VARCHAR(12) PRIMARY KEY,
    game_id VARCHAR(12) NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    template_team_id VARCHAR(12) REFERENCES game_template_teams(id),
    name TEXT NOT NULL,
    color TEXT,
    position INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL
);

-- Template invitations
CREATE TABLE game_template_invitations (
    id VARCHAR(12) PRIMARY KEY,
    template_id VARCHAR(12) NOT NULL REFERENCES game_templates(id) ON DELETE CASCADE,
    user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    group_id VARCHAR(12) REFERENCES groups(id) ON DELETE CASCADE,
    team_id VARCHAR(12) NOT NULL REFERENCES game_template_teams(id) ON DELETE CASCADE,
    created_at TIMESTAMP NOT NULL,
    CHECK ((user_id IS NOT NULL) != (group_id IS NOT NULL)),
    UNIQUE (template_id, user_id, group_id)
);

-- Game invitations table
CREATE TABLE game_invitations (
    game_id VARCHAR(12) NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    team_id VARCHAR(12) NOT NULL REFERENCES game_teams(id) ON DELETE CASCADE,
    group_id VARCHAR(12) REFERENCES groups(id) ON DELETE SET NULL,
    status invitation_status NOT NULL DEFAULT 'pending',
    invited_at TIMESTAMP NOT NULL,
    responded_at TIMESTAMP,
    PRIMARY KEY (game_id, user_id)
);

-- Group game invitations table (tracks when groups are invited to games)
CREATE TABLE group_game_invitations (
    game_id VARCHAR(12) NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    group_id VARCHAR(12) NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    invited_at TIMESTAMP NOT NULL,
    PRIMARY KEY (game_id, group_id)
);

-- Indexes for better performance
CREATE INDEX idx_games_template_id ON games(template_id);
CREATE INDEX idx_games_recurring_game_id ON games(recurring_game_id) WHERE recurring_game_id IS NOT NULL;
CREATE INDEX idx_games_scheduled_time ON games(scheduled_time);
CREATE INDEX idx_games_occurrence_date ON games(occurrence_date);
CREATE INDEX idx_game_templates_created_by ON game_templates(created_by_user_id);
CREATE INDEX idx_game_teams_game_id ON game_teams(game_id);
CREATE INDEX idx_game_teams_position ON game_teams(game_id, position);
CREATE INDEX idx_game_invitations_user_id ON game_invitations(user_id);
CREATE INDEX idx_game_invitations_status ON game_invitations(status);
CREATE INDEX idx_game_invitations_team_id ON game_invitations(team_id);
CREATE INDEX idx_group_game_invitations_group_id ON group_game_invitations(group_id);
CREATE INDEX idx_group_game_invitations_game_id ON group_game_invitations(game_id);
CREATE INDEX idx_recurring_games_template_id ON recurring_games(template_id);
CREATE INDEX idx_game_template_teams_template_id ON game_template_teams(template_id);
CREATE INDEX idx_game_template_invitations_template_id ON game_template_invitations(template_id);
