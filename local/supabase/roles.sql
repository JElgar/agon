-- Trimmed from Supabase's reference roles.sql (docker/volumes/db/roles.sql)
-- down to the roles this auth-only stack actually has/needs: `pgbouncer` and
-- `supabase_functions_admin` aren't even pre-created by this Postgres image
-- tag (we don't run a pooler or Functions), and an ALTER USER on a
-- nonexistent role aborts this whole init script — which then aborts every
-- init script after it (postgres's docker-entrypoint stops on the first
-- failure), silently skipping auth-ownership.sql too. Add roles back here
-- only once the matching service is actually added to docker-compose.yml.
\set pgpass `echo "$POSTGRES_PASSWORD"`

ALTER USER authenticator WITH PASSWORD :'pgpass';
ALTER USER supabase_auth_admin WITH PASSWORD :'pgpass';
