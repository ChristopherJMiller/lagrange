{ config, lib, pkgs, modulesPath, ... }:

{
  # Portable hardware bits only — partitioning + fileSystems are owned by
  # ./disko.nix. After install, `nixos-generate-config --no-filesystems` can
  # be run on the box to capture machine-specific detection if needed; the
  # defaults below are good enough for typical KVM-capable x86 hardware.
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
