CREATE TABLE credential_privacy_exceptions (
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    rule_id TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    PRIMARY KEY (thread_id, rule_id, fingerprint)
);
CREATE TABLE credential_privacy_epoch (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    digest TEXT NOT NULL
);
