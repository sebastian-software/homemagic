CREATE TABLE automation_event_cursor (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    cursor INTEGER NOT NULL CHECK (cursor >= 0),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    updated_at TEXT NOT NULL
) STRICT;

DROP INDEX automation_occurrences_recovery;

CREATE INDEX automation_occurrences_recovery
    ON automation_occurrences(state, occurred_at, event_cursor, id);
