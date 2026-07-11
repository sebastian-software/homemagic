CREATE TABLE automation_identities (
    id TEXT PRIMARY KEY,
    operational_state TEXT NOT NULL,
    active_version INTEGER,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE automation_drafts (
    automation_id TEXT PRIMARY KEY REFERENCES automation_identities(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE TABLE automation_versions (
    automation_id TEXT NOT NULL REFERENCES automation_identities(id) ON DELETE RESTRICT,
    version INTEGER NOT NULL CHECK (version > 0),
    state TEXT NOT NULL,
    document_hash TEXT NOT NULL CHECK (length(document_hash) = 64),
    plan_hash TEXT NOT NULL CHECK (length(plan_hash) = 64),
    registry_revision INTEGER NOT NULL CHECK (registry_revision >= 0),
    created_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (automation_id, version),
    UNIQUE (automation_id, document_hash),
    UNIQUE (automation_id, plan_hash)
) STRICT;

CREATE TABLE automation_approvals (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    decided_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (automation_id, version)
        REFERENCES automation_versions(automation_id, version) ON DELETE RESTRICT
) STRICT;

CREATE TABLE automation_occurrences (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    state TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    window_ends_at TEXT NOT NULL,
    event_cursor INTEGER,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (automation_id, version)
        REFERENCES automation_versions(automation_id, version) ON DELETE RESTRICT
) STRICT;

CREATE INDEX automation_occurrences_recovery
    ON automation_occurrences(state, occurred_at, id);

CREATE TABLE automation_runs (
    id TEXT PRIMARY KEY,
    automation_id TEXT NOT NULL,
    version INTEGER NOT NULL,
    occurrence_id TEXT NOT NULL UNIQUE REFERENCES automation_occurrences(id) ON DELETE RESTRICT,
    state TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (automation_id, version)
        REFERENCES automation_versions(automation_id, version) ON DELETE RESTRICT
) STRICT;

CREATE INDEX automation_runs_recovery ON automation_runs(state, updated_at, id);

CREATE TABLE automation_timers (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES automation_runs(id) ON DELETE RESTRICT,
    node_id INTEGER NOT NULL CHECK (node_id >= 0),
    state TEXT NOT NULL,
    ready_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

CREATE INDEX automation_timers_recovery ON automation_timers(state, ready_at, id);

CREATE TABLE automation_trace (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES automation_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    UNIQUE (run_id, sequence)
) STRICT;

CREATE INDEX automation_trace_order ON automation_trace(run_id, sequence);
