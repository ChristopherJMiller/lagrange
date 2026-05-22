-- Initial schema. One table per concern; simple enough that we don't need
-- foreign-key complications for v1.

CREATE TABLE IF NOT EXISTS repo_vms (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    name                  TEXT UNIQUE NOT NULL,
    repo_url              TEXT NOT NULL,
    branch                TEXT NOT NULL DEFAULT 'main',
    vm_ip                 TEXT NOT NULL UNIQUE,
    vm_mac                TEXT NOT NULL UNIQUE,
    vcpu                  INTEGER NOT NULL DEFAULT 4,
    mem_mb                INTEGER NOT NULL DEFAULT 4096,
    status                TEXT NOT NULL CHECK (status IN
                              ('provisioning','running','stopped','destroyed','failed')),
    created_at            TEXT NOT NULL,
    last_started_at       TEXT,
    last_stopped_at       TEXT,
    claude_session_name   TEXT
);

CREATE INDEX IF NOT EXISTS idx_repo_vms_status ON repo_vms(status);

CREATE TABLE IF NOT EXISTS ip_pool (
    ip       TEXT PRIMARY KEY,
    vm_name  TEXT REFERENCES repo_vms(name) ON DELETE SET NULL
);
