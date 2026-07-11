CREATE TABLE actors (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE actor_credentials (
    actor_id TEXT PRIMARY KEY REFERENCES actors(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    rotated_at TEXT NOT NULL
) STRICT;

CREATE TABLE actor_grants (
    id TEXT PRIMARY KEY,
    actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX actor_grants_actor ON actor_grants(actor_id, enabled);

CREATE TABLE commands (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE RESTRICT,
    idempotency_key TEXT NOT NULL,
    request_hash TEXT NOT NULL CHECK (
        length(request_hash) = 64 AND request_hash = lower(request_hash)
    ),
    state TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version >= 0),
    terminal INTEGER NOT NULL CHECK (terminal IN (0, 1)),
    received_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (actor_id, idempotency_key)
) STRICT;

CREATE INDEX commands_installation_updated
    ON commands(installation_id, updated_at DESC);
CREATE INDEX commands_recovery
    ON commands(terminal, updated_at, id);

CREATE TABLE command_audit (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    installation_id TEXT NOT NULL,
    command_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    from_state TEXT,
    to_state TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (command_id, sequence)
) STRICT;

CREATE INDEX command_audit_command_sequence
    ON command_audit(command_id, sequence);
CREATE INDEX command_audit_installation_occurred
    ON command_audit(installation_id, occurred_at DESC);
