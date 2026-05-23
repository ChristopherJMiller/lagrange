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
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICHR4q3amhKDhCF6+xa3oTXJX2ycN503+cEo/gpnOkFt git@chrismiller.xyz"
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
  time.timeZone = lib.mkDefault "America/Los_Angeles";
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
    # devpi-server's pytest suite contains mirror/streaming tests that fail
    # under the nixos-install sandbox (404s where 200 is expected). When the
    # installer can't pull the python wheel from a binary cache, those
    # failures gate the entire install. The wrapper `pkgs.devpi-server`
    # already has doCheck=false, but the underlying `python3Packages.devpi-*`
    # builds still run pytest — override them at the python package set so
    # the wrapper picks up tests-free versions. Upstream's tests don't tell
    # us anything we'd act on here.
    (final: prev: {
      python3 = prev.python3.override (old: {
        packageOverrides = lib.composeExtensions
          (old.packageOverrides or (_: _: { }))
          (pyfinal: pyprev: {
            devpi-server = pyprev.devpi-server.overridePythonAttrs (_: {
              doCheck = false;
              doInstallCheck = false;
            });
            devpi-common = pyprev.devpi-common.overridePythonAttrs (_: {
              doCheck = false;
              doInstallCheck = false;
            });
          });
      });
      python3Packages = final.python3.pkgs;
    })
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
