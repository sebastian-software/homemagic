DROP TABLE matter_fabric_stages;
DELETE FROM schema_migrations WHERE version = 9;
PRAGMA user_version = 8;
