-- Add up migration script here

-- Create enums for status fields
CREATE TYPE invitation_status AS ENUM ('pending', 'accepted', 'declined');
CREATE TYPE game_status AS ENUM ('scheduled', 'in_progress', 'completed', 'cancelled');
CREATE TYPE game_type AS ENUM ('football5_a_side', 'football11_a_side', 'basketball', 'tennis', 'badminton', 'cricket', 'rugby', 'hockey', 'other');

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

-- Games table
CREATE TABLE games (
    id VARCHAR(12) PRIMARY KEY,
    title TEXT NOT NULL,
    game_type game_type NOT NULL,
    location_latitude DECIMAL(10, 8) NOT NULL,
    location_longitude DECIMAL(11, 8) NOT NULL,
    location_name TEXT,
    scheduled_time TIMESTAMP NOT NULL,
    duration_minutes INTEGER NOT NULL,
    created_by_user_id TEXT NOT NULL REFERENCES users(id),
    created_at TIMESTAMP NOT NULL,
    status game_status NOT NULL DEFAULT 'scheduled'
);

-- Game teams table
CREATE TABLE game_teams (
    id VARCHAR(12) PRIMARY KEY,
    game_id VARCHAR(12) NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    color TEXT,
    position INTEGER NOT NULL,
    created_at TIMESTAMP NOT NULL
);

-- Game invitations table
CREATE TABLE game_invitations (
    game_id VARCHAR(12) NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id),
    team_id VARCHAR(12) NOT NULL REFERENCES game_teams(id) ON DELETE CASCADE,
    status invitation_status NOT NULL DEFAULT 'pending',
    invited_at TIMESTAMP NOT NULL,
    responded_at TIMESTAMP,
    PRIMARY KEY (game_id, user_id)
);

-- Indexes for better performance
CREATE INDEX idx_games_created_by ON games(created_by_user_id);
CREATE INDEX idx_games_scheduled_time ON games(scheduled_time);
CREATE INDEX idx_game_teams_game_id ON game_teams(game_id);
CREATE INDEX idx_game_teams_position ON game_teams(game_id, position);
CREATE INDEX idx_game_invitations_user_id ON game_invitations(user_id);
CREATE INDEX idx_game_invitations_status ON game_invitations(status);
CREATE INDEX idx_game_invitations_team_id ON game_invitations(team_id);

