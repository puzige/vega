CREATE TABLE projects (
  id TEXT PRIMARY KEY,            -- ulid
  path TEXT NOT NULL UNIQUE,      -- 绝对路径
  name TEXT NOT NULL,
  git_default_branch TEXT,
  created_at INTEGER NOT NULL,    -- unix ms
  last_opened_at INTEGER NOT NULL
);

CREATE TABLE threads (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id),
  title TEXT NOT NULL DEFAULT '',
  mode TEXT NOT NULL DEFAULT 'execute',   -- ask|plan|execute
  permission_mode TEXT NOT NULL DEFAULT 'confirm',  -- readonly|confirm|auto
  model TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'active',  -- active|archived
  pinned INTEGER NOT NULL DEFAULT 0,
  unread INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_threads_project ON threads(project_id, updated_at DESC);

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id),
  seq INTEGER NOT NULL,           -- 线程内单调递增
  role TEXT NOT NULL,             -- user|assistant|system
  kind TEXT NOT NULL DEFAULT 'text',  -- text|plan|error|summary
  content TEXT NOT NULL,          -- markdown 原文（完整，非增量）
  status TEXT NOT NULL DEFAULT 'done',  -- streaming|done|interrupted|failed
  created_at INTEGER NOT NULL,
  UNIQUE(thread_id, seq)
);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,            -- 与 provider 的 tool_use_id 对齐
  thread_id TEXT NOT NULL,
  message_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  tool TEXT NOT NULL,             -- bash|read|write|edit|glob|grep|web_fetch
  input_json TEXT NOT NULL,
  output_text TEXT,               -- 截断后展示文本
  output_full_path TEXT,          -- 完整输出落盘路径（大输出）
  status TEXT NOT NULL,           -- pending_approval|approved|rejected|running|success|failed|cancelled
  approval TEXT,                  -- once|always|deny + note
  exit_code INTEGER,
  duration_ms INTEGER,
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE INDEX idx_tool_calls_thread ON tool_calls(thread_id, seq);

CREATE TABLE token_usage (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  thread_id TEXT NOT NULL,
  message_id TEXT,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  cache_write_tokens INTEGER NOT NULL DEFAULT 0,
  cost_microcents INTEGER NOT NULL,   -- 成本引擎计算结果
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_usage_thread ON token_usage(thread_id);
CREATE INDEX idx_usage_day ON token_usage((created_at/86400000));  -- 仪表盘聚合

CREATE TABLE permissions (        -- 「总是允许」的规则记忆
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  pattern TEXT NOT NULL,          -- 如 "bash:cargo *" / "write:src/**"
  created_at INTEGER NOT NULL,
  UNIQUE(project_id, tool, pattern)
);
