{ nixpkgs
, microvm
, claude-code
, system ? "x86_64-linux"
}:

# mkRepoVm: build a NixOS configuration for one repo-VM.
#
# Used by the admin service in imperative mode: it generates a tiny per-VM
# flake at /var/lib/microvms/<name>/flake.nix that imports this lagrange
# repo as an input and calls mkRepoVm with the per-VM args.
#
# Hypervisor note: cloud-hypervisor, NOT firecracker. firecracker is the
# leanest microVM but cannot virtiofs-mount /nix/store. We need that share
# to keep VMs sub-second-warm-boot and avoid duplicating the store per-VM.

{ name
, repoUrl
, branch ? "main"
, vmIp
, vmMac
, vcpu ? 4
, memMb ? 4096
, balloonMb ? null
  # Default to the operator's published key so freshly-created VMs are
  # debuggable over SSH (port 22 on the VM's bridge IP, 10.42.0.X) without
  # the admin service having to thread an SSH key through. Override per-VM
  # by passing `operatorSshKey = null` to disable, or another key to swap.
, operatorSshKey ? "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAICHR4q3amhKDhCF6+xa3oTXJX2ycN503+cEo/gpnOkFt git@chrismiller.xyz"
}:

nixpkgs.lib.nixosSystem {
  inherit system;

  specialArgs = {
    inherit microvm claude-code;
    repoArgs = {
      inherit name repoUrl branch vmIp vmMac vcpu memMb operatorSshKey;
    };
  };

  modules = [
    microvm.nixosModules.microvm

    ({ config, pkgs, lib, repoArgs, ... }: {
      system.stateVersion = "25.11";

      ###### microVM configuration
      microvm = {
        hypervisor = "cloud-hypervisor";
        vcpu = repoArgs.vcpu;
        mem = repoArgs.memMb;
        balloon = true;

        shares = [
          # Read-only host /nix/store.
          {
            source = "/nix/store";
            mountPoint = "/nix/.ro-store";
            tag = "ro-store";
            proto = "virtiofs";
          }
          # Read-only shared agent config (CLAUDE.md, skills, commands).
          {
            source = "/var/lib/agent-shared";
            mountPoint = "/shared";
            tag = "shared";
            proto = "virtiofs";
          }
          # Read-write per-repo persistent volume.
          {
            source = "/var/lib/agent-state/${repoArgs.name}";
            mountPoint = "/persistent";
            tag = "persistent";
            proto = "virtiofs";
          }
        ];

        # Cloud-hypervisor supports only `tap` and `macvtap`, not the
        # higher-level `bridge` type. We create a tap and slave it to
        # cachebr0 ourselves via the tap-up hook below.
        interfaces = [{
          type = "tap";
          id = "vm-${repoArgs.name}";
          mac = repoArgs.vmMac;
        }];

        # microvm.nix's tap-up creates the tap and brings it up, but
        # doesn't bridge it. Attach the tap to cachebr0 after creation
        # so the guest sees the cache bridge subnet (and via NAT, the
        # outside world).
        binScripts.tap-up = lib.mkAfter ''
          ${pkgs.iproute2}/bin/ip link set 'vm-${repoArgs.name}' master cachebr0
        '';
      };

      ###### Guest networking
      # systemd's predictable-interface naming gives cloud-hypervisor's
      # virtio-net device a name like `enp0s4`, but our networking.interfaces
      # config targets `eth0`. Force legacy naming so the names line up.
      boot.kernelParams = [ "net.ifnames=0" ];

      networking = {
        hostName = repoArgs.name;
        useDHCP = false;
        interfaces.eth0.ipv4.addresses = [{
          address = repoArgs.vmIp;
          prefixLength = 24;
        }];
        defaultGateway = {
          address = "10.42.0.1";
          interface = "eth0";
        };
        nameservers = [ "10.42.0.1" ];
        # Host nftables controls egress. Guest firewall would just be noise.
        firewall.enable = false;
      };

      ###### Toolchain
      nixpkgs.overlays = [ claude-code.overlays.default ];
      nixpkgs.config.allowUnfreePredicate = pkg:
        builtins.elem (lib.getName pkg) [ "claude-code" ];

      environment.systemPackages = with pkgs; [
        claude-code
        git
        gh
        nodejs_22
        python3
        rustup
        go
        ripgrep
        fd
        bat
        jq
        tmux
        htop
      ];

      ###### Agent user (passwordless sudo; blast radius is the VM)
      users.users.agent = {
        isNormalUser = true;
        home = "/home/agent";
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = lib.optional
          (repoArgs.operatorSshKey != null)
          repoArgs.operatorSshKey;
      };
      security.sudo.wheelNeedsPassword = false;

      ###### Mount layout
      # Persistent state lives on the host under /var/lib/agent-state/<name>/,
      # mounted at /persistent inside the VM via virtiofs. We bind a few
      # subdirectories into the agent's home so `claude` reads/writes them
      # directly without symlink traversal. The set is also encoded in
      # admin-service/src/vm.rs::PERSISTENT_SUBDIRS — keep both in lockstep.
      systemd.tmpfiles.rules = [
        "d  /home/agent/.claude           0755 agent users -"
        "L+ /home/agent/.claude/CLAUDE.md - - - - /shared/CLAUDE.md"
        "L+ /home/agent/.claude/skills    - - - - /shared/skills"
        "L+ /home/agent/.claude/commands  - - - - /shared/commands"

        "d /persistent/projects 0755 agent users -"
        "d /persistent/todos    0755 agent users -"
        "d /persistent/statsig  0755 agent users -"
        "d /persistent/ssh      0700 agent users -"
        "d /persistent/work     0755 agent users -"
        "f /persistent/gitconfig 0644 agent users -"
      ];

      fileSystems = lib.mapAttrs'
        (target: src: lib.nameValuePair target {
          device = "/persistent/${src}";
          fsType = "none";
          options = [ "bind" ];
        })
        {
          "/home/agent/.claude/projects" = "projects";
          "/home/agent/.claude/todos" = "todos";
          "/home/agent/.claude/statsig" = "statsig";
          "/home/agent/.ssh" = "ssh";
          "/home/agent/work" = "work";
          "/home/agent/.gitconfig" = "gitconfig";
        };

      ###### Package-manager cache routing
      environment.variables = {
        CARGO_NET_GIT_FETCH_WITH_CLI = "true";
        npm_config_registry = "http://cache.internal:4873/";
        GOPROXY = "http://cache.internal:3000,direct";
        UV_INDEX_URL = "http://cache.internal:3141/root/pypi/+simple/";
        PIP_INDEX_URL = "http://cache.internal:3141/root/pypi/+simple/";
      };

      nix.settings.substituters = [ "http://cache.internal:8080/lagrange" ];
      nix.settings.trusted-public-keys = [
        # Public key for the lagrange attic cache. Populated post-bootstrap;
        # placeholder is deliberately invalid.
        "lagrange:REPLACE_WITH_ATTIC_PUBLIC_KEY="
      ];

      # Cargo config: route through nginx HTTP cache on cache.internal:7878.
      environment.etc."skel/.cargo/config.toml".text = ''
        [source.crates-io]
        replace-with = "lagrange"

        [source.lagrange]
        registry = "sparse+http://cache.internal:7878/index/"
      '';

      ###### claude remote-control session
      # `claude remote-control` requires a controlling TTY. systemd's
      # Type=simple doesn't allocate one — we used to wrap in tmux, but
      # tmux itself fails with "open terminal failed: not a terminal" when
      # invoked without a tty. Use `script -qc` instead: it creates a
      # ptmx/pts pair and runs claude inside it. systemd's main process
      # is now `script`, which stays around for the lifetime of claude.
      # Single-session mode (--spawn session), NOT worktree.
      systemd.services.claude-remote = {
        description = "Claude Code remote control session for ${repoArgs.name}";
        after = [ "network-online.target" "home-agent-work.mount" ];
        wants = [ "network-online.target" ];
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "simple";
          User = "agent";
          WorkingDirectory = "/home/agent/work";
          Environment = [
            "HOME=/home/agent"
            "TERM=screen-256color"
          ];
          # The admin service writes CLAUDE_CODE_OAUTH_TOKEN here on the host
          # at `<agent_state_dir>/agent.env`; virtiofs surfaces it as
          # /persistent/agent.env inside the guest. Leading `-` makes the
          # file optional so VMs created before the token is configured
          # still boot (Claude Code just prompts for login in that case).
          EnvironmentFile = "-/persistent/agent.env";
          ExecStart = pkgs.writeShellScript "claude-remote-start" ''
            set -euo pipefail
            cd /home/agent/work
            # Clone on first run if work/ is empty.
            if [ ! -d .git ]; then
              GIT_SSH_COMMAND="ssh -i /home/agent/.ssh/id_ed25519 -o StrictHostKeyChecking=accept-new" \
                ${pkgs.git}/bin/git clone --branch ${repoArgs.branch} ${repoArgs.repoUrl} .
            fi
            exec ${pkgs.util-linux}/bin/script -qc \
              "${pkgs.claude-code}/bin/claude remote-control --name ${repoArgs.name} --spawn session" \
              /dev/null
          '';
          Restart = "on-failure";
          RestartSec = 30;
        };
      };

      services.openssh = {
        enable = lib.mkDefault (repoArgs.operatorSshKey != null);
        settings.PasswordAuthentication = false;
      };

      ###### Resource accounting — give a second OOM fence inside the VM.
      systemd.slices."claude.slice".sliceConfig = {
        MemoryHigh = "${toString (repoArgs.memMb * 80 / 100)}M";
      };
      systemd.services.claude-remote.serviceConfig.Slice = "claude.slice";
    })
  ];
}
