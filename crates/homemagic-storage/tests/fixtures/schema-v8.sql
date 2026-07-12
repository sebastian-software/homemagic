DROP TABLE matter_operation_node_results;
DROP TABLE matter_fabric_stages;
DELETE FROM schema_migrations WHERE version IN (9, 10);
PRAGMA user_version = 8;
