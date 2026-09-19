-- Issue #74: local Skills authority. No Skill file or reference bytes enter
-- consent/audit rows. `skill_run_snapshots.bytes` is private thread data.
-- This migration is additive and independent of #73's 0012 tables.

CREATE TABLE skill_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  global_enabled INTEGER NOT NULL DEFAULT 0 CHECK (global_enabled IN (0, 1)),
  automatic_enabled INTEGER NOT NULL DEFAULT 0 CHECK (automatic_enabled IN (0, 1)),
  consent_generation INTEGER NOT NULL DEFAULT 0 CHECK (consent_generation >= 0),
  revocation_generation INTEGER NOT NULL DEFAULT 0 CHECK (revocation_generation >= 0)
);
INSERT INTO skill_settings (id) VALUES (1);

CREATE TABLE skill_project_settings (
  project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  automatic INTEGER NOT NULL DEFAULT 0 CHECK (automatic IN (0, 1))
);

CREATE TABLE skill_sources (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL CHECK (scope IN ('project', 'vega_global', 'imported')),
  project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
  configured_root TEXT NOT NULL CHECK (length(configured_root) BETWEEN 1 AND 4096),
  canonical_root TEXT NOT NULL CHECK (length(canonical_root) BETWEEN 1 AND 4096),
  root_dev TEXT NOT NULL CHECK (length(root_dev) BETWEEN 1 AND 20),
  root_ino TEXT NOT NULL CHECK (length(root_ino) BETWEEN 1 AND 20),
  import_order INTEGER NOT NULL DEFAULT 0 CHECK (import_order >= 0),
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  automatic INTEGER NOT NULL DEFAULT 0 CHECK (automatic IN (0, 1)),
  created_at INTEGER NOT NULL,
  CHECK ((scope = 'project') = (project_id IS NOT NULL))
);
CREATE UNIQUE INDEX idx_skill_source_project
  ON skill_sources(project_id) WHERE scope = 'project';
CREATE UNIQUE INDEX idx_skill_source_vega_global
  ON skill_sources(scope) WHERE scope = 'vega_global';
CREATE UNIQUE INDEX idx_skill_source_import_identity
  ON skill_sources(root_dev, root_ino) WHERE scope = 'imported';
CREATE INDEX idx_skill_source_order
  ON skill_sources(scope, import_order, canonical_root, id);
-- Also covers project removal's FK cascade: a root disappearing through a
-- different production service must revoke in-flight Skill authority.
CREATE TRIGGER skill_source_delete_revokes AFTER DELETE ON skill_sources
BEGIN
  SELECT CASE WHEN
    (SELECT consent_generation FROM skill_settings WHERE id = 1) >= 9223372036854775807 OR
    (SELECT revocation_generation FROM skill_settings WHERE id = 1) >= 9223372036854775807
    THEN RAISE(ABORT, 'skill generation exhausted') END;
  UPDATE skill_settings SET
    consent_generation = consent_generation + 1,
    revocation_generation = revocation_generation + 1
  WHERE id = 1;
END;

CREATE TABLE skill_approvals (
  source_id TEXT NOT NULL REFERENCES skill_sources(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
  approved_sha256 TEXT NOT NULL CHECK (length(approved_sha256) = 64),
  source_label TEXT NOT NULL CHECK (length(source_label) BETWEEN 1 AND 64),
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  automatic INTEGER NOT NULL DEFAULT 0 CHECK (automatic IN (0, 1)),
  reviewed_at INTEGER NOT NULL,
  PRIMARY KEY (source_id, name)
);

CREATE TABLE thread_skill_pins (
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  scope TEXT NOT NULL CHECK (scope IN ('project', 'vega_global', 'imported')),
  canonical_root TEXT NOT NULL CHECK (length(canonical_root) BETWEEN 1 AND 4096),
  root_dev TEXT NOT NULL CHECK (length(root_dev) BETWEEN 1 AND 20),
  root_ino TEXT NOT NULL CHECK (length(root_ino) BETWEEN 1 AND 20),
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
  approved_sha256 TEXT NOT NULL CHECK (length(approved_sha256) = 64),
  source_label TEXT NOT NULL CHECK (length(source_label) BETWEEN 1 AND 64),
  pinned_at INTEGER NOT NULL,
  PRIMARY KEY (thread_id, name)
);

CREATE TABLE skill_run_snapshots (
  run_id TEXT PRIMARY KEY,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  consent_generation INTEGER NOT NULL CHECK (consent_generation >= 0),
  revocation_generation INTEGER NOT NULL CHECK (revocation_generation >= 0),
  catalog_sha256 TEXT NOT NULL CHECK (length(catalog_sha256) = 64),
  snapshot_sha256 TEXT NOT NULL CHECK (length(snapshot_sha256) = 64),
  bytes BLOB NOT NULL CHECK (length(bytes) BETWEEN 1 AND 1048576),
  updated_at INTEGER NOT NULL
);
CREATE INDEX idx_skill_run_snapshots_thread
  ON skill_run_snapshots(thread_id, updated_at DESC);

CREATE TABLE skill_activation_audits (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id TEXT NOT NULL,
  thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 64),
  source_scope TEXT CHECK (source_scope IN ('project', 'vega_global', 'imported')),
  content_sha256 TEXT CHECK (content_sha256 IS NULL OR length(content_sha256) = 64),
  origin TEXT NOT NULL CHECK (origin IN ('model', 'explicit_user')),
  status TEXT NOT NULL CHECK (length(status) BETWEEN 1 AND 32),
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_skill_activation_audits_thread
  ON skill_activation_audits(thread_id, id);
