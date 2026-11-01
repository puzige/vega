CREATE TABLE mcp_config_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    generation INTEGER NOT NULL DEFAULT 0,
    accepted_digest BLOB,
    pending_base_digest BLOB,
    pending_declarations TEXT,
    CHECK ((pending_base_digest IS NULL) = (pending_declarations IS NULL))
);
INSERT INTO mcp_config_state (singleton) VALUES (1);

CREATE TABLE mcp_config_identities (
    id TEXT PRIMARY KEY,
    declaration TEXT NOT NULL,
    projected_revision INTEGER NOT NULL,
    tombstoned INTEGER NOT NULL DEFAULT 0 CHECK (tombstoned IN (0, 1))
);
