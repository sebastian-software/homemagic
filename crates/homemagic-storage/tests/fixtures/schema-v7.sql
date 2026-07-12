DROP TABLE matter_operation_node_results;
DROP TABLE matter_fabric_stages;
DROP TABLE matter_operation_bindings;
DELETE FROM schema_migrations WHERE version IN (8, 9, 10);
PRAGMA user_version = 7;
