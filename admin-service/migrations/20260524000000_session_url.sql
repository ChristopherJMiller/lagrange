-- claude_session_url: deep link to the operator's claude.ai/code session
-- for this VM. Populated by the guest's claude-session-publisher unit (which
-- scrapes the `claude remote-control` typescript on startup and POSTs the
-- URL back to the admin service over the cache bridge), and surfaced on the
-- VmDto so the orbit frontend can render a one-click "drive agent" link.

ALTER TABLE repo_vms ADD COLUMN claude_session_url TEXT;
