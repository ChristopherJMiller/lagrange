{ lib, ... }:

{
  # Disk layout for the satellite. Owned by `disko` — it provisions partitions
  # at install time and exports the matching `fileSystems.*` entries to the
  # running config, so we no longer keep placeholders in hardware.nix.
  #
  # The device path is `mkDefault` so `disko-install --disk main /dev/<X>` can
  # override it per-run without editing this file. Filesystem labels (`boot`,
  # `nixos`) are kept stable so any external tooling that referenced
  # /dev/disk/by-label/* still works.
  disko.devices.disk.main = {
    type = "disk";
    device = lib.mkDefault "/dev/nvme0n1";
    content = {
      type = "gpt";
      partitions = {
        ESP = {
          size = "1G";
          type = "EF00";
          content = {
            type = "filesystem";
            format = "vfat";
            mountpoint = "/boot";
            mountOptions = [ "umask=0077" ];
            extraArgs = [ "-n" "boot" ];
          };
        };
        root = {
          size = "100%";
          content = {
            type = "filesystem";
            format = "ext4";
            mountpoint = "/";
            extraArgs = [ "-L" "nixos" ];
          };
        };
      };
    };
  };
}
