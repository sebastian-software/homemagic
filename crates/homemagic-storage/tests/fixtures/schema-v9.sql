DROP TABLE matter_operation_node_results;
DELETE FROM schema_migrations WHERE version = 10;
PRAGMA user_version = 9;
