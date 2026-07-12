CREATE TABLE matter_fabric_stages (
    fabric_id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (installation_id)
) STRICT;

CREATE INDEX matter_fabric_stages_recovery
    ON matter_fabric_stages(state, updated_at, fabric_id);
