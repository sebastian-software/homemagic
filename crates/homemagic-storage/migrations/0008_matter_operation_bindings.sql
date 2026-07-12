CREATE TABLE matter_operation_bindings (
    operation_id TEXT PRIMARY KEY
        REFERENCES matter_operations(id) ON DELETE CASCADE,
    actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    action TEXT NOT NULL,
    idempotency_key TEXT NOT NULL CHECK (
        length(idempotency_key) BETWEEN 1 AND 128
    ),
    request_hash TEXT NOT NULL CHECK (
        length(request_hash) = 64 AND request_hash = lower(request_hash)
    ),
    policy_version INTEGER NOT NULL CHECK (policy_version > 0),
    payload_json TEXT NOT NULL,
    UNIQUE (actor_id, idempotency_key)
) STRICT;

CREATE INDEX matter_operation_bindings_actor
    ON matter_operation_bindings(actor_id, operation_id);
