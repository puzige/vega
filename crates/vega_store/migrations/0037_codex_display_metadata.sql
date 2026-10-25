CREATE TABLE codex_display_metadata (
    tool_call_id TEXT PRIMARY KEY NOT NULL REFERENCES tool_calls(id) ON DELETE CASCADE,
    preview_kind TEXT NOT NULL CHECK (preview_kind IN ('object', 'array', 'string', 'number', 'bool', 'null')),
    preview_count INTEGER,
    terminal_reason TEXT CHECK (terminal_reason IN (
        'permission_rejected', 'permission_invalid', 'permission_ended', 'permission_incomplete',
        'tool_completed', 'tool_failed', 'tool_cancelled', 'tool_incomplete'
    )),
    CHECK (
        (preview_kind IN ('object', 'array') AND preview_count IS NOT NULL AND preview_count BETWEEN 0 AND 262144)
        OR (preview_kind IN ('string', 'number', 'bool', 'null') AND preview_count IS NULL)
    )
) STRICT;
