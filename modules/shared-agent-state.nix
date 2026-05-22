{ config, lib, pkgs, ... }:

# Syncs the in-repo `shared-agent-state/` tree into /var/lib/agent-shared/ on
# the host. Repo-VMs mount that directory read-only via virtiofs and symlink
# its entries into ~/.claude/ (CLAUDE.md, skills/, commands/) inside the guest.
#
# Because the source is just files in the flake, comin picks up edits on the
# next reconcile and the new content appears in every VM with no restart —
# the VM-side mount is read-only virtiofs of a host directory.

let
  src = ../shared-agent-state;
in
{
  # Materialize the tree into the Nix store...
  environment.etc."agent-shared".source = src;

  # ...and mirror it into /var/lib/agent-shared/ so VMs can virtiofs-share a
  # path that doesn't depend on the current system generation's /etc layout.
  # The mirror is a symlink; the actual content lives in the store and is
  # GC-rooted by the system closure.
  systemd.tmpfiles.rules = [
    "L+ /var/lib/agent-shared - - - - /etc/agent-shared"
  ];
}
