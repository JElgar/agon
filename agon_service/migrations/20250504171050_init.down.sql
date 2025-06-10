-- Add down migration script here
DROP TABLE IF EXISTS game_invitations;
DROP TABLE IF EXISTS games;
DROP TABLE IF EXISTS team_members;
DROP TABLE IF EXISTS teams;
DROP TABLE IF EXISTS users;
DROP TYPE IF EXISTS invitation_status;
DROP TYPE IF EXISTS game_status;
DROP TYPE IF EXISTS game_type;
