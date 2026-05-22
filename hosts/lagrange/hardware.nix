{ config, lib, pkgs, modulesPath, ... }:

{
  # Real hardware.nix lives on the satellite itself after `nixos-generate-config`.
  # This file is a placeholder that captures only what's portable; the live
  # satellite must overwrite it with the generated copy committed to git.
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
  ];

  boot = {
    loader = {
      systemd-boot.enable = true;
      efi.canTouchEfiVariables = true;
    };

    initrd = {
      availableKernelModules = [ "xhci_pci" "ahci" "nvme" "usb_storage" "sd_mod" ];
      kernelModules = [ ];
    };

    kernelModules = [ "kvm-intel" "kvm-amd" ];
    extraModulePackages = [ ];

    kernelParams = [
      # vfio-pci.ids = "10de:..." — uncomment when a GPU is added.
    ];
  };

  # Placeholder filesystems. nixos-generate-config will replace this with real
  # UUIDs.
  fileSystems."/" = lib.mkDefault {
    device = "/dev/disk/by-label/nixos";
    fsType = "ext4";
  };

  fileSystems."/boot" = lib.mkDefault {
    device = "/dev/disk/by-label/boot";
    fsType = "vfat";
  };

  swapDevices = lib.mkDefault [ ];

  # Tune for a single x86_64 desktop with NVMe; KVM-friendly defaults.
  hardware.cpu.intel.updateMicrocode = lib.mkDefault true;
  hardware.cpu.amd.updateMicrocode = lib.mkDefault true;
  hardware.enableRedistributableFirmware = true;

  nixpkgs.hostPlatform = lib.mkDefault "x86_64-linux";

  # Discrete GPU passthrough is a v2 concern; just keep KMS off for the would-be
  # passthrough card by default. No-op until the user enables it.
  powerManagement.cpuFreqGovernor = lib.mkDefault "performance";
}
