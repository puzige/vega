CREATE TABLE assistant_run_activity_members (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    CHECK (length(message_id) BETWEEN 1 AND 128),
    CHECK (length(run_id) BETWEEN 1 AND 128)
);

CREATE INDEX assistant_run_activity_members_run
    ON assistant_run_activity_members(run_id, message_id);

WITH RECURSIVE steer_edges(thread_id, previous_message_id, message_id) AS (
    SELECT pending.thread_id, previous.id, assistant.id
    FROM pending_messages pending
    JOIN messages user_message
        ON user_message.id = pending.id
        AND user_message.thread_id = pending.thread_id
        AND user_message.role = 'user'
        AND user_message.status = 'done'
    JOIN messages previous
        ON previous.thread_id = user_message.thread_id
        AND previous.seq = user_message.seq - 1
        AND previous.role = 'assistant'
    JOIN messages assistant
        ON assistant.thread_id = user_message.thread_id
        AND assistant.seq = user_message.seq + 1
        AND assistant.role = 'assistant'
    WHERE pending.behavior = 'steer'
        AND pending.state = 'delivered'
), activity_members(message_id, run_id) AS (
    SELECT edge.previous_message_id, edge.previous_message_id
    FROM steer_edges edge
    WHERE NOT EXISTS (
        SELECT 1
        FROM steer_edges previous_edge
        WHERE previous_edge.message_id = edge.previous_message_id
    )
    UNION ALL
    SELECT edge.message_id, activity_members.run_id
    FROM steer_edges edge
    JOIN activity_members ON activity_members.message_id = edge.previous_message_id
)
INSERT OR IGNORE INTO assistant_run_activity_members(message_id, run_id)
SELECT message_id, run_id FROM activity_members;
