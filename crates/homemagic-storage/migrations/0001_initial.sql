CREATE TABLE installations (
    id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE integrations (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    adapter TEXT NOT NULL,
    instance_key TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (installation_id, adapter, instance_key)
) STRICT;

CREATE TABLE spaces (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES spaces(id) ON DELETE SET NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE devices (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES installations(id) ON DELETE CASCADE,
    integration_id TEXT NOT NULL REFERENCES integrations(id) ON DELETE RESTRICT,
    native_id TEXT NOT NULL,
    lifecycle TEXT NOT NULL,
    availability TEXT NOT NULL,
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    last_success TEXT,
    payload_json TEXT NOT NULL,
    UNIQUE (integration_id, native_id)
) STRICT;

CREATE TABLE endpoints (
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    endpoint_id TEXT NOT NULL,
    name TEXT,
    PRIMARY KEY (device_id, endpoint_id)
) STRICT;

CREATE TABLE capabilities (
    device_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    name TEXT NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    risk TEXT NOT NULL,
    descriptor_json TEXT NOT NULL,
    snapshot_json TEXT,
    PRIMARY KEY (device_id, endpoint_id, name, version),
    FOREIGN KEY (device_id, endpoint_id)
        REFERENCES endpoints(device_id, endpoint_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE device_aliases (
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    alias TEXT NOT NULL,
    PRIMARY KEY (device_id, alias)
) STRICT;

CREATE TABLE device_spaces (
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    space_id TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    PRIMARY KEY (device_id, space_id)
) STRICT;

CREATE TABLE observations (
    device_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    capability_name TEXT NOT NULL,
    capability_version INTEGER NOT NULL,
    received_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (device_id, endpoint_id, capability_name, capability_version),
    FOREIGN KEY (device_id, endpoint_id)
        REFERENCES endpoints(device_id, endpoint_id) ON DELETE CASCADE
) STRICT;

CREATE TABLE events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX events_device_cursor ON events(device_id, cursor);
CREATE INDEX events_occurred_at ON events(occurred_at);

CREATE TABLE repairs (
    id TEXT PRIMARY KEY,
    device_id TEXT REFERENCES devices(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    closed_at TEXT,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX repairs_status_created ON repairs(status, created_at);
