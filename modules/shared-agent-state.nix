{ config, lib, pkgs, ... }:

# Syncs the in-repo `shared-agent-state/` tree into /var/lib/agent-shared/ on
# the host. Repo-VMs mount that directory read-only via virtiofs and symlink
# its entries into ~/.claude/ (CLAUDE.md, skills/, commands/) inside the guest.
#
# Pre-orbit design: /var/lib/agent-shared was a symlink to /etc/agent-shared,
# meaning every file was store-managed and could only change via comin.
#
# With orbit: skills/ and commands/ stay store-managed (gitops via this repo),
# but CLAUDE.md is a real writable file under lagrange-admin's ownership.
# It's seeded from /etc/agent-shared/CLAUDE.md on first boot (the in-repo
# version becomes the seed), then orbit's PUT /v1/agent/claude-md owns the
# runtime contents. Re-running activation won't clobber the operator's edits.
#
# Running VMs see CLAUDE.md edits within ~5s (virtiofs propagates host
# writes live), but `claude` caches the file at session start — vessels
# need a restart for the agent to actually re-read it.

let
  src = ../shared-agent-state;
in
{
  # Materialize the canonical tree into the Nix store at /etc/agent-shared.
  environment.etc."agent-shared".source = src;

  systemd.tmpfiles.rules = [
    # Container dir, group-owned by lagrange-admin so the admin service can
    # atomically replace CLAUDE.md (its sibling .tmp + rename needs write
    # access to the dir).
    "d /var/lib/agent-shared           0755 root lagrange-admin -"

    # Sub-trees that should always reflect the in-repo state. Symlink, not
    # copy — operator edits to these directories belong in git.
    "L+ /var/lib/agent-shared/skills   - - - - /etc/agent-shared/skills"
    "L+ /var/lib/agent-shared/commands - - - - /etc/agent-shared/commands"

    # Seed CLAUDE.md from the store version on FIRST boot (and only first
    # boot — `C` copies if missing, doesn't replace if present). After
    # that, orbit owns the file. To reset to the in-repo default, the
    # operator deletes the live file and activates again, or uses a
    # future "reset to seed" button in orbit.
    "C /var/lib/agent-shared/CLAUDE.md 0644 lagrange-admin lagrange-admin - /etc/agent-shared/CLAUDE.md"
  ];
}
