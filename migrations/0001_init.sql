-- Singleton row (guild_id is the only env-provided value; everything else is Discord-configured)
CREATE TABLE guild_config (
    guild_id                TEXT PRIMARY KEY,
    reports_channel_id     TEXT,
    mod_role_id             TEXT,
    vc_strategy             TEXT NOT NULL DEFAULT 'most_recent_activity',
    buffer_duration_secs    INTEGER NOT NULL DEFAULT 60,
    post_report_tail_secs   INTEGER NOT NULL DEFAULT 15,
    consent_reminder_text   TEXT,
    updated_at              TEXT NOT NULL
);

CREATE TABLE report_categories (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id    TEXT NOT NULL,
    label       TEXT NOT NULL,
    value       TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    active      INTEGER NOT NULL DEFAULT 1,
    UNIQUE(guild_id, value)
);

CREATE TABLE user_consent (
    user_id             TEXT PRIMARY KEY,
    state               TEXT NOT NULL, -- 'pending' | 'granted' (no row at all == unknown)
    granted_at          TEXT,
    last_reminder_at    TEXT,
    updated_at          TEXT NOT NULL
);

CREATE TABLE voice_activity_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    channel_id  TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    event_type  TEXT NOT NULL, -- 'join' | 'leave' | 'move' | 'consent_granted'
    at          TEXT NOT NULL
);
CREATE INDEX idx_voice_activity_channel_at ON voice_activity_log(channel_id, at);
CREATE INDEX idx_voice_activity_user_at ON voice_activity_log(user_id, at);

CREATE TABLE reports (
    id                          TEXT PRIMARY KEY,
    reporter_id                 TEXT NOT NULL,
    reported_user_id            TEXT NOT NULL,
    channel_id                  TEXT NOT NULL,
    category_id                 INTEGER REFERENCES report_categories(id),
    category_label_snapshot     TEXT NOT NULL,
    details_text                TEXT NOT NULL,
    has_audio                   INTEGER NOT NULL DEFAULT 0,
    audio_dir                   TEXT,
    transcript_json              TEXT,
    status                       TEXT NOT NULL DEFAULT 'pending',
    report_message_id            TEXT,
    created_at                   TEXT NOT NULL,
    finalized_at                  TEXT
);

CREATE TABLE report_participants (
    report_id   TEXT NOT NULL REFERENCES reports(id),
    user_id     TEXT NOT NULL,
    role        TEXT NOT NULL, -- 'reporter' | 'reported' | 'bystander_recorded'
    PRIMARY KEY (report_id, user_id)
);

CREATE TABLE moderator_decisions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id       TEXT NOT NULL REFERENCES reports(id),
    moderator_id    TEXT NOT NULL,
    decision        TEXT NOT NULL, -- 'action_taken' | 'no_action_taken' | 'dismissed'
    note            TEXT,
    decided_at      TEXT NOT NULL
);
