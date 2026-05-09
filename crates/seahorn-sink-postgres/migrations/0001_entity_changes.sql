CREATE TABLE IF NOT EXISTS entity_changes (
    id               BIGSERIAL PRIMARY KEY,
    entity_type      TEXT        NOT NULL,
    entity_id        TEXT        NOT NULL,
    slot             BIGINT      NOT NULL,
    tx_signature     TEXT,
    commitment_status TEXT       NOT NULL,
    fields           JSONB,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_ec_type_slot ON entity_changes (entity_type, slot);
CREATE INDEX IF NOT EXISTS idx_ec_slot      ON entity_changes (slot);
CREATE INDEX IF NOT EXISTS idx_ec_status    ON entity_changes (commitment_status);

CREATE TABLE IF NOT EXISTS cursors (
    name         TEXT PRIMARY KEY,
    cursor_bytes BYTEA NOT NULL,
    slot         BIGINT NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
