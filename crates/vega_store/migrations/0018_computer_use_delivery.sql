CREATE TABLE computer_screenshot_artifacts (
    call_id TEXT PRIMARY KEY REFERENCES tool_calls(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    FOREIGN KEY (message_id, ordinal) REFERENCES image_attachments(message_id, ordinal) ON DELETE CASCADE,
    UNIQUE (message_id, ordinal)
);
CREATE TABLE builtin_skill_activations (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (name = 'vega-cua'),
    version INTEGER NOT NULL CHECK (version > 0),
    content_sha256 TEXT NOT NULL CHECK (length(content_sha256) = 64),
    PRIMARY KEY (message_id, name)
);
