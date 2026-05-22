{ config, pkgs, lib, inputs, ... }:

let
  microvmCli = inputs.microvm.packages.${pkgs.system}.microvm;
in
{
  imports = [
    ./hardware.nix
    ./networking.nix
    ./comin.nix
    ./secrets.nix
  ];

  system.stateVersion = "25.11";

  networking.hostName = "lagrange";

  # Single Linux user. Isolation between repos comes from microVMs, not OS-level
  # accounts.
  users.users.chris = {
    isNormalUser = true;
    extraGroups = [ "wheel" "kvm" "libvirtd" "microvm" ];
    openssh.authorizedKeys.keys = [
      # Replace with operator's real key after bootstrap.
      "ssh-ed25519 AAAA__PLACEHOLDER__ chris@workstation"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      KbdInteractiveAuthentication = false;
      PermitRootLogin = "no";
    };
  };

  # Time, locale, console.
  time.timeZone = lib.mkDefault "Etc/UTC";
  i18n.defaultLocale = "en_US.UTF-8";
  console.keyMap = "us";

  # KVM + microvm prerequisites.
  boot.kernelModules = [ "kvm-intel" "kvm-amd" "tun" "vhost_net" ];

  # Substituters: pull from the operator's cluster cache too once it exists.
  nix = {
    settings = {
      experimental-features = [ "nix-command" "flakes" ];
      trusted-users = [ "root" "@wheel" "chris" ];
      auto-optimise-store = true;
      substituters = [
        "https://cache.nixos.org"
        "https://nix-community.cachix.org"
        "https://microvm.cachix.org"
        "https://claude-code.cachix.org"
      ];
      trusted-public-keys = [
        "cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY="
        "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
        "microvm.cachix.org-1:oXnBc6hRE3eX5rSYdRyMYXnfzcCxC7yKPTbZXALsqys="
        "claude-code.cachix.org-1:YeXf2aNu7UTX8Vwrze0za1WEDS+4DuI2kVeWEE4fsRk="
      ];
    };
    gc = {
      automatic = true;
      dates = "weekly";
      options = "--delete-older-than 7d";
    };
    optimise.automatic = true;
  };

  # Modules below contribute to this set.
  nixpkgs.overlays = [
    inputs.claude-code.overlays.default
    inputs.attic.overlays.default
  ];
  nixpkgs.config.allowUnfreePredicate = pkg:
    builtins.elem (lib.getName pkg) [ "claude-code" ];

  # Quality-of-life packages on the host (deliberately small).
  environment.systemPackages = with pkgs; [
    git
    htop
    btop
    iotop
    tmux
    vim
    jq
    ripgrep
    sqlite
    age
    sops
    wireguard-tools
    nftables
    microvmCli
  ];

  # /var/lib/agent-state is owned by the lagrange-admin module (see
  # modules/lagrange-admin.nix); /var/lib/lagrange-admin likewise. Avoid
  # restating their tmpfiles rules here to prevent ownership/mode drift.
  systemd.tmpfiles.rules = [
    "d /var/lib/agent-shared 0755 root  root  -"
    "d /var/lib/cache        0755 root  root  -"
    "d /var/lib/microvms     0755 microvm microvm -"
  ];
}
