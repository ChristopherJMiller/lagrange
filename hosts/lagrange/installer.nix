{ config, lib, pkgs, modulesPath, ... }:

{
  # Minimal installer ISO. Carries:
  #   - the operator's SSH public key (so they can finish provisioning remotely)
  #   - comin's repo URL pre-baked
  #   - wireguard-tools (the actual private key lands via sops after first boot)
  #
  # After `nixos-install --flake .#lagrange` reboots into the real system,
  # comin owns the box — no more SSH-driven imperative steps.

  imports = [
    "${modulesPath}/installer/cd-dvd/installation-cd-minimal.nix"
  ];

  image.baseName = lib.mkForce "lagrange-installer";
  isoImage = {
    makeEfiBootable = true;
    makeUsbBootable = true;
  };

  networking.hostName = "lagrange-installer";

  users.users.root.openssh.authorizedKeys.keys = [
    # Replace with operator's real key before building the ISO.
    "ssh-ed25519 AAAA__INSTALLER_BOOTSTRAP_KEY__ chris@workstation"
  ];

  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "prohibit-password";
  };

  # Tools needed during install.
  environment.systemPackages = with pkgs; [
    git
    wireguard-tools
    sops
    age
    nixos-install-tools
    parted
    gptfdisk
  ];

  # Convenience banner so the operator knows what they're looking at.
  services.getty.helpLine = lib.mkForce ''
    Lagrange installer ISO. Provision with:
        nixos-install --flake github:christopherjmiller/lagrange#lagrange
    After install, drop your sops age key at /mnt/var/lib/sops-nix/key.txt
    and reboot. comin will take over from there.
  '';
}
