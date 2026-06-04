-- Initial PhoneBridge schema.
-- All timestamps are Unix epoch SECONDS (not ms) on the daemon side to
-- match SQLite's built-in functions; the wire protocol uses ms.

CREATE TABLE IF NOT EXISTS devices (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    device_id   TEXT    NOT NULL UNIQUE,    -- UUIDv4
    public_key  TEXT    NOT NULL,           -- base64 of SubjectPublicKeyInfo
    last_seen   INTEGER NOT NULL,           -- epoch seconds
    paired      INTEGER NOT NULL DEFAULT 0  -- boolean
);

CREATE INDEX IF NOT EXISTS devices_last_seen_idx ON devices(last_seen DESC);

CREATE TABLE IF NOT EXISTS pairings (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id           TEXT    NOT NULL UNIQUE,
    cert_pem            TEXT    NOT NULL,
    cert_fingerprint    TEXT    NOT NULL,  -- colon-separated upper hex
    paired_at           INTEGER NOT NULL,  -- epoch seconds
    FOREIGN KEY (device_id) REFERENCES devices(device_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id       TEXT    NOT NULL,
    connected_at    INTEGER NOT NULL,      -- epoch seconds
    disconnected_at INTEGER,                -- null while online
    peer_addr       TEXT    NOT NULL,
    FOREIGN KEY (device_id) REFERENCES devices(device_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS sessions_device_idx ON sessions(device_id, connected_at DESC);
CREATE INDEX IF NOT EXISTS sessions_active_idx ON sessions(device_id) WHERE disconnected_at IS NULL;

CREATE TABLE IF NOT EXISTS notifications (
    id              TEXT    NOT NULL,       -- per-device notification id
    device_id       TEXT    NOT NULL,
    package_name    TEXT    NOT NULL,
    app_name        TEXT,
    title           TEXT    NOT NULL,
    content         TEXT    NOT NULL,
    posted_at       INTEGER NOT NULL,       -- epoch ms
    is_sensitive    INTEGER NOT NULL DEFAULT 0,
    category        TEXT,
    read            INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (id, device_id),
    FOREIGN KEY (device_id) REFERENCES devices(device_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS notifications_posted_at_idx ON notifications(posted_at DESC);
CREATE INDEX IF NOT EXISTS notifications_device_idx ON notifications(device_id, posted_at DESC);

CREATE TABLE IF NOT EXISTS sms_messages (
    id               TEXT    NOT NULL,
    device_id        TEXT    NOT NULL,
    sim_slot         INTEGER,
    phone_number     TEXT    NOT NULL,
    body             TEXT    NOT NULL,
    direction        TEXT    NOT NULL,      -- 'in' or 'out'
    timestamp        INTEGER NOT NULL,      -- epoch ms
    PRIMARY KEY (id, device_id),
    FOREIGN KEY (device_id) REFERENCES devices(device_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS sms_timestamp_idx ON sms_messages(timestamp DESC);
CREATE INDEX IF NOT EXISTS sms_conversation_idx ON sms_messages(device_id, phone_number, timestamp DESC);

CREATE TABLE IF NOT EXISTS calls (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    device_id       TEXT    NOT NULL,
    phone_number    TEXT    NOT NULL,
    contact_name    TEXT,
    state           TEXT    NOT NULL,        -- ringing / offhook / idle
    started_at      INTEGER NOT NULL,        -- epoch ms
    ended_at        INTEGER,                 -- epoch ms
    direction       TEXT    NOT NULL,        -- incoming / outgoing / missed
    duration_secs   INTEGER,
    sim_slot        INTEGER,
    FOREIGN KEY (device_id) REFERENCES devices(device_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS calls_started_at_idx ON calls(started_at DESC);
CREATE INDEX IF NOT EXISTS calls_device_idx ON calls(device_id, started_at DESC);

CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp       INTEGER NOT NULL,        -- epoch ms
    device_id       TEXT,
    event           TEXT    NOT NULL,        -- 'pair.success', 'ws.closed', ...
    detail          TEXT                     -- optional JSON
);

CREATE INDEX IF NOT EXISTS audit_log_ts_idx ON audit_log(timestamp DESC);
