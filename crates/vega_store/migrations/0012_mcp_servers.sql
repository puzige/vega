-- Issue #73: configuration metadata only. Credential values remain in the
-- owner-only keystore; no provider key or OAuth token belongs in SQLite.
CREATE TABLE mcp_servers (
  id TEXT PRIMARY KEY,
  display_name TEXT NOT NULL,
  transport TEXT NOT NULL CHECK (transport IN ('local', 'remote')),
  local_executable TEXT,
  local_args_json TEXT,
  local_working_directory TEXT,
  local_env_refs_json TEXT,
  remote_endpoint TEXT,
  remote_allow_loopback_http INTEGER NOT NULL DEFAULT 0
    CHECK (remote_allow_loopback_http IN (0, 1)),
  remote_auth_mode TEXT CHECK (remote_auth_mode IN ('none', 'bearer', 'oauth')),
  remote_credential_ref TEXT,
  remote_oauth_issuer TEXT,
  remote_oauth_client_id TEXT,
  enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
  deleting INTEGER NOT NULL DEFAULT 0 CHECK (deleting IN (0, 1)),
  config_revision INTEGER NOT NULL DEFAULT 1 CHECK (config_revision > 0),
  last_error_code TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (deleting = 0 OR enabled = 0),
  CHECK (
    (transport = 'local' AND local_executable IS NOT NULL AND local_args_json IS NOT NULL
      AND local_env_refs_json IS NOT NULL AND remote_endpoint IS NULL
      AND remote_auth_mode IS NULL AND remote_credential_ref IS NULL
      AND remote_oauth_issuer IS NULL AND remote_oauth_client_id IS NULL)
    OR
    (transport = 'remote' AND remote_endpoint IS NOT NULL AND remote_auth_mode IS NOT NULL
      AND local_executable IS NULL AND local_args_json IS NULL
      AND local_working_directory IS NULL AND local_env_refs_json IS NULL)
  )
);
CREATE INDEX idx_mcp_servers_enabled ON mcp_servers(enabled, created_at, id);

-- Durable cleanup outbox. DB mutations enqueue obsolete server-owned slots
-- in the same transaction that revokes their authority. A crash after local
-- deletion but before ACK is harmless: keystore Missing is idempotent.
CREATE TABLE mcp_secret_cleanup (
  credential_ref TEXT PRIMARY KEY,
  server_id TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_mcp_secret_cleanup_server ON mcp_secret_cleanup(server_id);
