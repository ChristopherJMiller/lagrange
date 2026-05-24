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
    # First-boot console password so we're not locked out if SSH is wedged
    # or the workstation key is unavailable. Change with `passwd` after
    # login — NixOS doesn't re-apply initialPassword once /etc/shadow has a
    # real hash.
    initialPassword = "lagrange";
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

  # Console auto-login as `chris` on tty1. The box lives in a physically
  # controlled spot and the value at the console is recovery access during
  # bring-up — losing it would mean a USB reboot to fix every misconfig.
  # SSH still requires the operator's authorized key.
  services.getty.autologinUser = "chris";

  # Time, locale, console.
  time.timeZone = lib.mkDefault "America/Los_Angeles";
  i18n.defaultLocale = "en_US.UTF-8";
  console.keyMap = "us";

  # KVM + microvm prerequisites. kvm-amd vs kvm-intel is set in
  # ./hardware.nix (currently kvm-amd for the Ryzen 4600G).
  boot.kernelModules = [ "tun" "vhost_net" ];

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
    # already has doCheck=false, but the underlying `python3Packages.devpi-server`
    # build still runs pytest — override at the python package set so the
    # wrapper picks up a tests-free version. Upstream's tests don't tell us
    # anything we'd act on here.
    (final: prev: {
      python3 = prev.python3.override (old: {
        packageOverrides = lib.composeExtensions
          (old.packageOverrides or (_: _: { }))
          (pyfinal: pyprev: {
            devpi-server = pyprev.devpi-server.overridePythonAttrs (_: {
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
    # 0775 microvm:kvm so lagrange-admin (added to the kvm group in
    # modules/lagrange-admin.nix) can write per-VM directories without
    # sudo. `z` (not `d`) so the perms are reapplied to an existing dir
    # left over from earlier installs that used different ownership.
    "z /var/lib/microvms     0775 microvm kvm -"
  ];
}
