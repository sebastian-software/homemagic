DROP TABLE matter_operation_bindings;
DELETE FROM schema_migrations WHERE version = 8;
PRAGMA user_version = 7;
