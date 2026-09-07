CREATE TABLE sidebar_organization (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    preferences TEXT NOT NULL DEFAULT '{"view":"Projects","project_view":"ByProject","sort":"Updated"}',
    collapsed TEXT NOT NULL DEFAULT '[]'
);
INSERT INTO sidebar_organization(singleton) VALUES (1);
CREATE TABLE sidebar_groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT NOT NULL,
    position INTEGER NOT NULL
);
CREATE TABLE sidebar_memberships (
    thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL REFERENCES sidebar_groups(id) ON DELETE CASCADE,
    position INTEGER NOT NULL
);
CREATE INDEX sidebar_memberships_order ON sidebar_memberships(group_id, position, thread_id);
CREATE TABLE sidebar_project_order (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    position INTEGER NOT NULL
);
