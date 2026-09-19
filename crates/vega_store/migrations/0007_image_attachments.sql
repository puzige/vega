-- Issue 63 R5: attachment ownership is identical to the durable user turn.
CREATE TABLE image_attachments (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0 AND ordinal < 4),
    encoded BLOB NOT NULL CHECK (length(encoded) > 0 AND length(encoded) <= 8388608),
    PRIMARY KEY (message_id, ordinal)
);
