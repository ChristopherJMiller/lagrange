-- Multi-account GitHub PAT support.
--
-- Replaces the previous singleton `state_dir/github-token` with a named
-- collection at `state_dir/github-accounts/<alias>`. The DB table is the
-- index — file existence on disk is the source of truth for "is this
-- token actually staged right now". This keeps file/db drift recoverable:
-- if someone deletes the file out-of-band the row goes stale but no
-- foreign keys break.
--
-- Each repo_vm picks one account (or none — leaves gh.env empty so the
-- guest's claude-remote.service starts with no GITHUB_TOKEN). On startup
-- the admin service migrates an existing singleton into a `default`
-- alias (Rust side, since file ops aren't sql).

CREATE TABLE IF NOT EXISTS github_accounts (
    alias       TEXT PRIMARY KEY,
    created_at  TEXT NOT NULL
);

-- Per-VM PAT assignment. NULL means "no PAT — agent.env has no GITHUB_TOKEN".
ALTER TABLE repo_vms ADD COLUMN github_account TEXT
  REFERENCES github_accounts(alias) ON DELETE SET NULL;
