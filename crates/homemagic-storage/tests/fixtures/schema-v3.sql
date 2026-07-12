DROP TABLE automation_event_cursor;
DROP INDEX automation_occurrences_recovery;
CREATE INDEX automation_occurrences_recovery
    ON automation_occurrences(state, occurred_at, id);
DELETE FROM schema_migrations WHERE version IN (4, 5);
PRAGMA user_version = 3;
