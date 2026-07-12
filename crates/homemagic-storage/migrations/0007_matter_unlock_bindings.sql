CREATE TABLE matter_unlock_authorization_bindings (
    authorization_id TEXT PRIMARY KEY
        REFERENCES matter_unlock_authorizations(id) ON DELETE CASCADE,
    request_hash TEXT NOT NULL CHECK (
        length(request_hash) = 64 AND request_hash = lower(request_hash)
    ),
    device_id TEXT NOT NULL,
    endpoint_id TEXT NOT NULL,
    capability_schema TEXT NOT NULL CHECK (capability_schema = 'access_control.v1'),
    action TEXT NOT NULL CHECK (action = 'unlock')
) STRICT;

CREATE INDEX matter_unlock_bindings_target
    ON matter_unlock_authorization_bindings(
        device_id, endpoint_id, capability_schema, action
    );
