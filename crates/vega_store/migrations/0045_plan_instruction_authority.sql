CREATE TABLE plan_instruction_authority (
    message_id TEXT PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    permission_fence TEXT NOT NULL CHECK (json_valid(permission_fence))
);
