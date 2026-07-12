CREATE TABLE matter_operation_node_results (
    operation_id TEXT PRIMARY KEY
        REFERENCES matter_operations(id) ON DELETE CASCADE,
    fabric_id TEXT NOT NULL,
    node_id INTEGER NOT NULL CHECK (node_id > 0),
    device_id TEXT NOT NULL REFERENCES devices(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY (fabric_id, node_id)
        REFERENCES matter_nodes(fabric_id, node_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX matter_operation_node_results_node
    ON matter_operation_node_results(fabric_id, node_id, operation_id);
