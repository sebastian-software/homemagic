ALTER TABLE events RENAME TO events_device_only;

CREATE TABLE events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    device_id TEXT REFERENCES devices(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL
) STRICT;

INSERT INTO events(cursor, id, device_id, occurred_at, payload_json)
SELECT cursor, id, device_id, occurred_at, payload_json
FROM events_device_only
ORDER BY cursor;

DROP TABLE events_device_only;

CREATE INDEX events_device_cursor ON events(device_id, cursor);
CREATE INDEX events_occurred_at ON events(occurred_at);
