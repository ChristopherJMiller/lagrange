-- Multi-account Sentry MCP OAuth-bundle storage.
--
-- Each row indexes a per-org Sentry OAuth bundle the operator obtained
-- on their laptop (by letting `claude` complete its OAuth handshake
-- against mcp.sentry.dev, then extracting mcpServers.sentry.oauth from
-- ~/.claude.json). The bundle itself lives at
-- `state_dir/sentry-accounts/<alias>.json`; the DB table is just the
-- index. File existence on disk is the source of truth for "really
-- staged", same convention as github_accounts.
--
-- Per-VM assignment is optional; NULL means the vessel has no Sentry
-- MCP server configured (mcpServers.sentry is omitted from the staged
-- .claude.json).

CREATE TABLE IF NOT EXISTS sentry_accounts (
    alias       TEXT PRIMARY KEY,
    created_at  TEXT NOT NULL
);

ALTER TABLE repo_vms ADD COLUMN sentry_account TEXT
  REFERENCES sentry_accounts(alias) ON DELETE SET NULL;
