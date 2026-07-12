CREATE TABLE matter_fabrics (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX matter_fabrics_installation
    ON matter_fabrics(installation_id, updated_at, id);

CREATE TABLE matter_nodes (
    fabric_id TEXT NOT NULL REFERENCES matter_fabrics(id) ON DELETE CASCADE,
    node_id INTEGER NOT NULL CHECK (node_id > 0),
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE RESTRICT,
    descriptor_revision INTEGER NOT NULL CHECK (descriptor_revision > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (fabric_id, node_id),
    UNIQUE (fabric_id, device_id)
) STRICT;

CREATE TABLE matter_endpoints (
    fabric_id TEXT NOT NULL,
    node_id INTEGER NOT NULL,
    endpoint_number INTEGER NOT NULL CHECK (
        endpoint_number >= 0 AND endpoint_number <= 65535
    ),
    descriptor_json TEXT NOT NULL,
    PRIMARY KEY (fabric_id, node_id, endpoint_number),
    FOREIGN KEY (fabric_id, node_id)
        REFERENCES matter_nodes(fabric_id, node_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE matter_projections (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    fabric_id TEXT NOT NULL,
    node_id INTEGER NOT NULL,
    endpoint_number INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    capability_schema TEXT NOT NULL,
    projection_revision INTEGER NOT NULL CHECK (projection_revision > 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    converged INTEGER NOT NULL CHECK (converged IN (0, 1)),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (fabric_id, node_id, endpoint_number, capability_schema),
    FOREIGN KEY (fabric_id, node_id, endpoint_number)
        REFERENCES matter_endpoints(fabric_id, node_id, endpoint_number) ON DELETE CASCADE,
    FOREIGN KEY (device_id, endpoint_id)
        REFERENCES endpoints(device_id, endpoint_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX matter_projections_recovery
    ON matter_projections(installation_id, converged, updated_at, id);

CREATE TABLE matter_subscriptions (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    fabric_id TEXT NOT NULL,
    node_id INTEGER NOT NULL,
    state TEXT NOT NULL,
    report_sequence INTEGER NOT NULL CHECK (report_sequence >= 0),
    stale_after TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (fabric_id, node_id),
    FOREIGN KEY (fabric_id, node_id)
        REFERENCES matter_nodes(fabric_id, node_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX matter_subscriptions_recovery
    ON matter_subscriptions(installation_id, state, stale_after, id);

CREATE TABLE matter_operations (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    fabric_id TEXT NOT NULL REFERENCES matter_fabrics(id) ON DELETE RESTRICT,
    phase TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    terminal INTEGER NOT NULL CHECK (terminal IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX matter_operations_recovery
    ON matter_operations(installation_id, terminal, updated_at, id);

CREATE TABLE matter_operation_progress (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL REFERENCES matter_operations(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision > 0),
    phase TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (operation_id, revision)
) STRICT;

CREATE TABLE matter_repairs (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    operation_id TEXT NOT NULL REFERENCES matter_operations(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX matter_repairs_recovery
    ON matter_repairs(installation_id, status, updated_at, id);

CREATE TABLE matter_unlock_authorizations (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE RESTRICT,
    command_id TEXT NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    requesting_actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
    approving_actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
    projection_id TEXT NOT NULL REFERENCES matter_projections(id) ON DELETE CASCADE,
    desired_revision INTEGER NOT NULL CHECK (desired_revision > 0),
    policy_revision INTEGER NOT NULL CHECK (policy_revision > 0),
    issued_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    CHECK (expires_at > issued_at)
) STRICT;

CREATE INDEX matter_unlock_authorizations_retention
    ON matter_unlock_authorizations(installation_id, consumed_at, expires_at);

CREATE TABLE matter_desired_command_slots (
    projection_id TEXT PRIMARY KEY REFERENCES matter_projections(id) ON DELETE CASCADE,
    desired_revision INTEGER NOT NULL CHECK (desired_revision > 0),
    command_id TEXT NOT NULL REFERENCES commands(id) ON DELETE RESTRICT,
    dispatched_at TEXT,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE matter_command_supersessions (
    old_command_id TEXT PRIMARY KEY REFERENCES commands(id) ON DELETE CASCADE,
    new_command_id TEXT NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    projection_id TEXT NOT NULL REFERENCES matter_projections(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    CHECK (old_command_id <> new_command_id)
) STRICT;

CREATE INDEX matter_command_supersessions_new
    ON matter_command_supersessions(new_command_id, occurred_at);
