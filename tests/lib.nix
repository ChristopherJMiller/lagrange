{ lib, ... }:

# Base config shared by every VM test. Trims the closure aggressively so a
# cold-cache test build doesn't drag in man pages, info docs, the
# documentation generator, or the full kernel headers tree.
#
# Drops roughly half the build time off a cold-cache NixOS VM test.

{
  documentation.enable = false;
  documentation.man.enable = false;
  documentation.info.enable = false;
  documentation.doc.enable = false;
  documentation.nixos.enable = false;

  # The test harness doesn't need any of these.
  services.udisks2.enable = false;

  # NixOS test machines have low default memory; 2 GiB makes everything
  # past systemd unit start noticeably snappier without much closure cost.
  virtualisation.memorySize = lib.mkDefault 2048;
  virtualisation.diskSize = lib.mkDefault 4096;
  virtualisation.cores = lib.mkDefault 2;

  # Quieter boot.
  boot.consoleLogLevel = lib.mkDefault 3;
  boot.kernelParams = [ "quiet" ];
}
