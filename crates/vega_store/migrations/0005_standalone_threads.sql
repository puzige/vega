-- R15: a task may be standalone. Rebuild only tables whose foreign keys
-- point at threads so existing rows and child records remain intact.
-- The migration runner temporarily disables foreign-key enforcement while
-- the old tables are replaced; it restores enforcement before committing.

ALTER TABLE messages RENAME TO messages_r14;
ALTER TABLE sidebar_memberships RENAME TO sidebar_memberships_r14;
ALTER TABLE threads RENAME TO threads_r14;

CREATE TABLE threads (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES projects(id),
  title TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT 'execute',
  permission_mode TEXT NOT NULL DEFAULT 'confirm',
  model TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',
  pinned INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
INSERT INTO threads
  (id, project_id, title, mode, permission_mode, model, status, pinned, unread, created_at, updated_at)
SELECT id, project_id, title, mode, permission_mode, model, status, pinned, unread, created_at, updated_at
FROM threads_r14;

CREATE TABLE messages_new (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id),
  seq INTEGER NOT NULL,
  role TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'text',
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'done',
  created_at INTEGER NOT NULL,
  plan_status TEXT
    CHECK (
      plan_status IS NULL OR plan_status IN (
        'pending', 'approved', 'changes_requested', 'abandoned'
      )
    ),
  plan_review_note TEXT,
  plan_reviewed_at INTEGER,
  UNIQUE(thread_id, seq)
);
INSERT INTO messages_new
  (id, thread_id, seq, role, kind, content, status, created_at,
   plan_status, plan_review_note, plan_reviewed_at)
SELECT id, thread_id, seq, role, kind, content, status, created_at,
       plan_status, plan_review_note, plan_reviewed_at
FROM messages_r14;

CREATE TABLE sidebar_memberships_new (
  thread_id TEXT PRIMARY KEY REFERENCES threads(id) ON DELETE CASCADE,
  group_id TEXT NOT NULL REFERENCES sidebar_groups(id) ON DELETE CASCADE,
  position INTEGER NOT NULL
);
INSERT INTO sidebar_memberships_new (thread_id, group_id, position)
SELECT thread_id, group_id, position
FROM sidebar_memberships_r14;

DROP TABLE messages_r14;
DROP TABLE sidebar_memberships_r14;
DROP TABLE threads_r14;

ALTER TABLE messages_new RENAME TO messages;
ALTER TABLE sidebar_memberships_new RENAME TO sidebar_memberships;

CREATE INDEX idx_threads_project ON threads(project_id, updated_at DESC);
CREATE INDEX sidebar_memberships_order
  ON sidebar_memberships(group_id, position, thread_id);
