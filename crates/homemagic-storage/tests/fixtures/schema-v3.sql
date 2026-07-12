DROP TABLE matter_operation_bindings;
DROP TABLE matter_unlock_authorization_bindings;
DROP TABLE matter_command_supersessions;
DROP TABLE matter_desired_command_slots;
DROP TABLE matter_unlock_authorizations;
DROP TABLE matter_repairs;
DROP TABLE matter_operation_progress;
DROP TABLE matter_operations;
DROP TABLE matter_subscriptions;
DROP TABLE matter_projections;
DROP TABLE matter_endpoints;
DROP TABLE matter_nodes;
DROP TABLE matter_fabrics;

DROP TABLE automation_event_cursor;
DROP INDEX automation_occurrences_recovery;
CREATE INDEX automation_occurrences_recovery
    ON automation_occurrences(state, occurred_at, id);
DELETE FROM schema_migrations WHERE version IN (4, 5, 6, 7, 8);
PRAGMA user_version = 3;
