-- Typed views over entity_changes for cleaner PostgREST queries.
-- Run after seahorn has created the entity_changes table:
--   psql $DATABASE_URL -f docker/views.sql

CREATE OR REPLACE VIEW buys AS
SELECT
    slot,
    tx_signature,
    commitment_status,
    fields->>'mint'                          AS mint,
    fields->>'user'                          AS "user",
    (fields->>'token_amount')::bigint        AS token_amount,
    (fields->>'sol_cost')::bigint            AS sol_cost,
    created_at
FROM entity_changes
WHERE entity_type = 'Buy';

CREATE OR REPLACE VIEW sells AS
SELECT
    slot,
    tx_signature,
    commitment_status,
    fields->>'mint'                          AS mint,
    fields->>'user'                          AS "user",
    (fields->>'token_amount')::bigint        AS token_amount,
    (fields->>'sol_output')::bigint          AS sol_output,
    created_at
FROM entity_changes
WHERE entity_type = 'Sell';

CREATE OR REPLACE VIEW creates AS
SELECT
    slot,
    tx_signature,
    commitment_status,
    fields->>'mint'                          AS mint,
    fields->>'name'                          AS name,
    fields->>'symbol'                        AS symbol,
    fields->>'uri'                           AS uri,
    fields->>'creator'                       AS creator,
    created_at
FROM entity_changes
WHERE entity_type = 'Create';

GRANT SELECT ON buys, sells, creates TO web_anon;
