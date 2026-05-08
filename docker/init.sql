-- PostgREST anonymous read role
CREATE ROLE web_anon NOLOGIN;
GRANT USAGE ON SCHEMA public TO web_anon;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO web_anon;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO web_anon;

-- Typed views for convenient PostgREST queries
-- These are created after seahorn runs once and creates the entity_changes table.
-- To recreate: psql $DATABASE_URL -f docker/views.sql
