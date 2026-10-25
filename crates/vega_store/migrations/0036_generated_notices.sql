CREATE TABLE generated_notices (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    text_start_bytes INTEGER NOT NULL CHECK (text_start_bytes >= 0),
    text_end_bytes INTEGER NOT NULL CHECK (text_end_bytes > text_start_bytes),
    kind TEXT NOT NULL CHECK (kind IN ('turn_limit', 'computer_stopped')),
    turn_limit INTEGER,
    delivered_images INTEGER,
    PRIMARY KEY (message_id, text_start_bytes),
    CHECK (
        (kind = 'turn_limit' AND turn_limit IS NOT NULL AND turn_limit BETWEEN 1 AND 4294967295 AND delivered_images IS NULL)
        OR
        (kind = 'computer_stopped' AND turn_limit IS NULL AND delivered_images IS NOT NULL AND delivered_images IN (0, 1))
    )
) STRICT;
