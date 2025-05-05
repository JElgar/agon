-- Add up migration script here
CREATE TABLE users (
	id TEXT PRIMARY KEY,
	first_name TEXT NOT NULL,
	last_name TEXT NOT NULL,
	email TEXT NOT NULL,
	created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE teams (
	id VARCHAR(12) PRIMARY KEY,
	name TEXT NOT NULL,
	created_by_user_id TEXT REFERENCES users(id) NOT NULL,
	created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE team_members (
	user_id TEXT REFERENCES users(id),
	team_id VARCHAR(12) REFERENCES teams(id),
	PRIMARY KEY (user_id, team_id)
);

