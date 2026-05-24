-- Per-VM permission mode for `claude remote-control`.
--
--   'auto'              → claude remote-control --permission-mode auto
--                         (classifier-mediated approval; default for new VMs)
--   'dangerously-skip'  → claude --dangerously-skip-permissions remote-control
--                         (no approval gate; for trusted-environment vessels
--                         where the operator wants full autonomy)
--
-- The two flag positions matter: --dangerously-skip-permissions is a
-- top-level claude flag, not a remote-control subcommand flag.
--
-- The CHECK keeps DB corruption from rendering as a broken `claude` invocation
-- (an unknown value would otherwise be interpolated raw into the flake).

ALTER TABLE repo_vms
  ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'auto'
    CHECK (permission_mode IN ('auto', 'dangerously-skip'));
