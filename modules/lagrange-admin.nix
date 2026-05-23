{ config, lib, pkgs, self, ... }:

# Repo-VM lifecycle service. The admin service binds to a single interface
# (defaults to the WireGuard endpoint); the network layer is the primary
# defense and the bearer token is defense-in-depth.

let
  cfg = config.lagrange.admin;
  adminPkg = self.packages.${pkgs.system}.lagrange-admin;
in
{
  options.lagrange.admin = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    bindAddress = lib.mkOption {
      type = lib.types.str;
      default = "10.99.0.2";
      description = ''
        Bind address. In production this MUST be the WireGuard interface IP,
        never 0.0.0.0. Tests override to 127.0.0.1.
      '';
    };

    bindPort = lib.mkOption {
      type = lib.types.port;
      default = 8443;
    };

    stateDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/lagrange-admin";
    };

    agentStateRoot = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/agent-state";
    };

    microvmStateDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/microvms";
    };

    flakeRef = lib.mkOption {
      type = lib.types.str;
      default = "github:christopherjmiller/lagrange";
    };

    logLevel = lib.mkOption {
      type = lib.types.enum [ "trace" "debug" "info" "warn" "error" ];
      default = "info";
    };

    tokenFile = lib.mkOption {
      type = lib.types.path;
      default = config.sops.secrets.admin-service-token.path;
      defaultText = lib.literalExpression
        "config.sops.secrets.admin-service-token.path";
      description = ''
        File containing the bearer token. Defaults to the sops-managed
        secret; tests can override with a writeText path.
      '';
    };

    deployKeysTarFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = config.sops.secrets.deploy-keys-tar.path or null;
      defaultText = lib.literalExpression
        "config.sops.secrets.deploy-keys-tar.path";
    };

    requireWireguard = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Whether to gate the unit on wireguard-wg0.service. Tests turn this
        off so they don't need a working tunnel.
      '';
    };

    requireSops = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether to gate the unit on sops-nix.service. Our pinned sops-nix
        installs secrets via an activation script (not a systemd unit),
        so a Requires=sops-nix.service is unsatisfiable and prevents the
        admin service from starting at all. Activation runs before any
        unit, so the token is present at /run/secrets/admin-service-token
        by the time systemd brings lagrange-admin up. Tests already turn
        this off and supply a plaintext token via tokenFile.
      '';
    };

    trustedSsoPeer = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = "10.99.0.1";
      description = ''
        Source IP allowed to bypass the bearer-token check by presenting
        an `X-authentik-username` header. In production this is the
        cluster-side wg-gateway (10.99.0.1) — ingress-nginx applies the
        authentik forward-auth, strips client-supplied versions of the
        header, and forwards the trusted ones through the tunnel. The
        IP gate prevents any other pod from spoofing the header by
        talking directly to wg0. Set to null to disable SSO auth (tests
        do this so they can run against 127.0.0.1).
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.lagrange-admin = {
      isSystemUser = true;
      group = "lagrange-admin";
      home = cfg.stateDir;
      description = "Lagrange admin service";
      extraGroups = [ "microvm" ];
    };
    users.groups.lagrange-admin = { };

    systemd.services.lagrange-admin = {
      description = "Lagrange admin service (repo-VM lifecycle)";
      after = [ "network-online.target" ]
        ++ lib.optional cfg.requireWireguard "wireguard-wg0.service"
        ++ lib.optional cfg.requireSops "sops-nix.service";
      wants = [ "network-online.target" ];
      requires =
        lib.optional cfg.requireWireguard "wireguard-wg0.service"
        ++ lib.optional cfg.requireSops "sops-nix.service";
      wantedBy = [ "multi-user.target" ];

      path = with pkgs; [
        git
        openssh
        nix
        systemd
        coreutils
        util-linux
        # sudo is required by vm.rs::run_sudo for systemctl/microvm/nixos-rebuild
        # calls — without it the unit gets ENOENT before the privileged
        # command can run. The actual privilege gate is the sudoers
        # allowlist further down.
        sudo
        # The microvm CLI lives in the flake input, not nixpkgs. Without
        # it on PATH, `microvm -c <name>` fails (the sudoers allowlist
        # uses /run/current-system/sw/bin/microvm but sudo still needs
        # to resolve `microvm` to that path through PATH).
        self.inputs.microvm.packages.${pkgs.system}.microvm
      ];

      environment = {
        LAGRANGE_BIND = "${cfg.bindAddress}:${toString cfg.bindPort}";
        LAGRANGE_STATE_DIR = cfg.stateDir;
        LAGRANGE_AGENT_STATE_ROOT = cfg.agentStateRoot;
        LAGRANGE_MICROVM_DIR = cfg.microvmStateDir;
        LAGRANGE_FLAKE_REF = cfg.flakeRef;
        LAGRANGE_TOKEN_FILE = toString cfg.tokenFile;
        RUST_LOG = cfg.logLevel;
      } // lib.optionalAttrs (cfg.deployKeysTarFile != null) {
        LAGRANGE_DEPLOY_KEYS_TAR = toString cfg.deployKeysTarFile;
      } // lib.optionalAttrs (cfg.trustedSsoPeer != null) {
        LAGRANGE_TRUSTED_SSO_PEER = cfg.trustedSsoPeer;
      };

      serviceConfig = {
        Type = "simple";
        User = "lagrange-admin";
        Group = "lagrange-admin";
        ExecStart = "${adminPkg}/bin/lagrange-admin serve";
        Restart = "on-failure";
        RestartSec = 5;

        StateDirectory = "lagrange-admin";
        StateDirectoryMode = "0750";

        # The unit shells out to systemctl/microvm/nixos-rebuild via sudo, so
        # NoNewPrivileges has to stay false. The sudo allowlist (below) is
        # what actually constrains what the service can do as root.
        NoNewPrivileges = false;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ReadWritePaths = [
          cfg.stateDir
          cfg.agentStateRoot
          cfg.microvmStateDir
        ];
      };
    };

    security.sudo.extraRules = [{
      users = [ "lagrange-admin" ];
      commands = [
        { command = "${pkgs.systemd}/bin/systemctl"; options = [ "NOPASSWD" ]; }
        { command = "${pkgs.systemd}/bin/journalctl"; options = [ "NOPASSWD" ]; }
        { command = "${pkgs.systemd}/bin/machinectl"; options = [ "NOPASSWD" ]; }
        # Use store paths so PATH shadowing can't redirect the privileged
        # invocation to a different binary.
        { command = "/run/current-system/sw/bin/microvm"; options = [ "NOPASSWD" ]; }
        { command = "/run/current-system/sw/bin/nixos-rebuild"; options = [ "NOPASSWD" ]; }
      ];
    }];

    # Every path in serviceConfig.ReadWritePaths must already exist at unit
    # start, or systemd fails the mount namespace step with status 226. Create
    # all of them up front so the service is functional whether or not the
    # microvm.nix host module is in the closure (e.g. in tests).
    systemd.tmpfiles.rules = [
      "d ${cfg.stateDir}                  0750 lagrange-admin lagrange-admin -"
      "d ${cfg.stateDir}/deploy-keys      0700 lagrange-admin lagrange-admin -"
      "d ${cfg.stateDir}/vm-flakes        0750 lagrange-admin lagrange-admin -"
      "d ${cfg.agentStateRoot}            0750 lagrange-admin lagrange-admin -"
      "d ${cfg.microvmStateDir}           0755 root root -"
    ];
  };
}
