-- The supabase/postgres image pre-bakes a few `auth.*` helper functions
-- (uid(), role(), email()) owned by `postgres`, for RLS use even before
-- Auth ever runs. GoTrue's own migrations connect as `supabase_auth_admin`
-- and `CREATE OR REPLACE` those same functions — which fails with "must be
-- owner of function uid" unless `supabase_auth_admin` already owns them.
-- Real Supabase deployments don't hit this (their image/GoTrue pairing
-- keeps ownership in sync); reassigning it here up front is the
-- straightforward fix for this dev-only stack. Filename sorts after
-- roles.sql (which creates/passwords `supabase_auth_admin` in the first
-- place) so the role exists by the time this runs.
ALTER SCHEMA auth OWNER TO supabase_auth_admin;
DO $$
DECLARE
  f record;
BEGIN
  FOR f IN
    SELECT p.oid::regprocedure AS sig
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace
    WHERE n.nspname = 'auth'
  LOOP
    EXECUTE format('ALTER FUNCTION %s OWNER TO supabase_auth_admin', f.sig);
  END LOOP;
END $$;
