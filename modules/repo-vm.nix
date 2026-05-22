{ config, lib, pkgs, ... }:

# Host-side wiring for repo-VMs. The per-VM guest configs come from
# lib/repo-vm.nix and are stamped onto disk imperatively by the admin service.
# microvm.nix's interfaces[].bridge = "cachebr0" handles tap enslavement, so
# nothing further is needed here today. autostart is left empty; entries are
# added by the admin service after `microvm -c`.

{
  microvm.autostart = lib.mkDefault [ ];
}
