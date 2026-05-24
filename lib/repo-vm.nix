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
  # "auto" (classifier-mediated approval, default) or "dangerously-skip"
  # (no approval gate; for trusted-environment vessels). The two values
  # land at DIFFERENT positions in the claude invocation —
  # --permission-mode is a remote-control subcommand flag, but
  # --dangerously-skip-permissions is a top-level claude flag. The DB
  # CHECK in admin-service/migrations/20260524120000_permission_mode.sql
  # mirrors this enum.
, permissionMode ? "auto"
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
      inherit name repoUrl branch vmIp vmMac vcpu memMb operatorSshKey permissionMode;
    };
  };

  modules = [
    microvm.nixosModules.microvm

    ({ config, pkgs, lib, repoArgs, ... }: let
      # claude invocation differs by permission mode. --dangerously-skip-permissions
      # is a TOP-LEVEL claude flag (before the subcommand); --permission-mode
      # is a remote-control subcommand flag.
      claudeInvocation =
        if repoArgs.permissionMode == "dangerously-skip" then
          "${pkgs.claude-code}/bin/claude --dangerously-skip-permissions remote-control --name ${repoArgs.name} --spawn same-dir --verbose"
        else
          "${pkgs.claude-code}/bin/claude remote-control --name ${repoArgs.name} --spawn same-dir --permission-mode auto --verbose";
    in {
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

        # Bind-mount targets for the full-scope Claude Code credentials.
        # systemd-mount needs the target file to exist before it can be
        # bind-replaced; pre-create as 0600 owned by agent.
        "f /home/agent/.claude/.credentials.json 0600 agent users -"
        "f /home/agent/.claude.json              0600 agent users -"

        "d /persistent/projects 0755 agent users -"
        "d /persistent/todos    0755 agent users -"
        "d /persistent/statsig  0755 agent users -"
        "d /persistent/ssh      0700 agent users -"
        "d /persistent/work     0755 agent users -"
        "f /persistent/gitconfig 0644 agent users -"
        # Bind-mount sources for Claude credentials. The host admin
        # service writes real contents when credentials are POSTed; until
        # then these stay as empty placeholders.
        "f /persistent/credentials.json 0640 agent users -"
        "f /persistent/claude.json      0640 agent users -"
        # gh.env: GITHUB_TOKEN / GH_TOKEN (optional). The host writes
        # actual content when POST /v1/auth/github-token has been called;
        # if no token, the file is missing and the EnvironmentFile=- in
        # claude-remote.service handles that gracefully.
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
          # NOTE: .gitconfig is intentionally NOT bind-mounted. `git config
          # --global` uses atomic rename (.gitconfig.lock → .gitconfig),
          # which fails with EBUSY on bind-mounted single files. Instead
          # we set GIT_CONFIG_GLOBAL=/persistent/gitconfig in the
          # claude-remote service Environment so git treats the persistent
          # file as "global" and writes to it directly — rename works
          # because there's no mount in the way.
          "/home/agent/.claude/.credentials.json" = "credentials.json";
          "/home/agent/.claude.json" = "claude.json";
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
        # `gh auth setup-git` shells out to `git` via PATH (it doesn't
        # honor an explicit --git-path or similar). Systemd's default
        # PATH for services is minimal — without this the start script
        # fails with "unable to find git executable in PATH", crashloops,
        # and bindsTo on claude-session-publisher drags that down too.
        # Listing the tools the start script reaches for through PATH
        # rather than $store/bin: git, gh, openssh (for the SSH-key
        # fallback's clone), claude-code (for the exec).
        path = with pkgs; [ git gh openssh claude-code ];
        serviceConfig = {
          Type = "simple";
          User = "agent";
          WorkingDirectory = "/home/agent/work";
          Environment = [
            "HOME=/home/agent"
            "TERM=screen-256color"
            # Persist git config to /persistent/gitconfig directly
            # instead of bind-mounting ~/.gitconfig (which breaks the
            # atomic-rename `git config --global` uses, including the
            # `git config --global ...` call inside `gh auth setup-git`).
            "GIT_CONFIG_GLOBAL=/persistent/gitconfig"
          ];
          # Optional env files staged by the admin service. Leading `-`
          # makes each file optional so VMs whose corresponding host-side
          # secret hasn't been POSTed yet still boot:
          #   agent.env  — CLAUDE_CODE_OAUTH_TOKEN (inference-only token)
          #   gh.env     — GITHUB_TOKEN / GH_TOKEN for `git push`
          EnvironmentFile = [ "-/persistent/agent.env" "-/persistent/gh.env" ];
          ExecStart = pkgs.writeShellScript "claude-remote-start" ''
            set -euo pipefail
            cd /home/agent/work

            # If a github PAT is in env, prefer HTTPS+token over SSH for
            # both the first clone and any subsequent push/pull:
            #   1. `gh auth setup-git` writes a credential helper into
            #      ~/.gitconfig that hands the PAT to git on demand
            #   2. `insteadOf` rewrites git@github.com: URLs to https
            #      so the repo_url the admin passed (likely the SSH
            #      form copied from `gh repo view`) still resolves
            # Without a PAT we fall back to SSH using a deploy key the
            # operator dropped into /persistent/ssh.
            if [ -n "''${GITHUB_TOKEN:-}" ]; then
              ${pkgs.gh}/bin/gh auth setup-git
              ${pkgs.git}/bin/git config --global \
                url.https://github.com/.insteadOf git@github.com:
              ${pkgs.git}/bin/git config --global \
                url.https://github.com/.insteadOf ssh://git@github.com/
            fi

            # Clone on first run if work/ is empty.
            if [ ! -d .git ]; then
              if [ -n "''${GITHUB_TOKEN:-}" ]; then
                # HTTPS path — credential helper supplies the token,
                # no SSH host-key dance needed.
                ${pkgs.git}/bin/git clone --branch ${repoArgs.branch} ${repoArgs.repoUrl} .
              else
                GIT_SSH_COMMAND="ssh -i /home/agent/.ssh/id_ed25519 -o StrictHostKeyChecking=accept-new" \
                  ${pkgs.git}/bin/git clone --branch ${repoArgs.branch} ${repoArgs.repoUrl} .
              fi
            fi
            # `claude remote-control` (subcommand) — server mode. Per
            # Anthropic's docs at /en/remote-control, this registers a
            # session with claude.ai/code that the operator drives from
            # the web/mobile sidebar. Flags:
            #   --name           session title shown in claude.ai/code
            #   --spawn session  single-session mode (one VM = one session)
            #   --permission-mode auto   classifier-mediated approval
            #   --add-dir        pre-trust the workspace
            #   --verbose        surface registration errors (see
            #                     troubleshooting in the docs)
            #
            # script(1) captures the TUI (full of terminal escapes) to a
            # typescript file so an operator who ssh's in can read the
            # session URL/QR code claude prints on startup. The systemd
            # journal only sees `[NNB blob data]` lines for the same
            # output, which isn't useful.
            # Subcommand flags (per `claude remote-control --help`):
            #   --name STR              session title at claude.ai/code
            #   --permission-mode auto  classifier-mediated approval
            #   --verbose               registration error detail
            # NOT supported here: --add-dir, --dangerously-skip-permissions,
            # --sandbox — those are top-level claude flags only.
            # Default --spawn is `same-dir`, which pre-creates one session
            # in /home/agent/work and stays up across reconnects.
            # --spawn same-dir is the default mode AND skips the first-run
            # interactive prompt ("Pick same-dir or worktree"). Without
            # this flag, claude blocks indefinitely waiting on keyboard
            # input that never comes.
            exec ${pkgs.util-linux}/bin/script -q \
              -c "${claudeInvocation}" \
              /tmp/claude-remote.typescript
          '';
          Restart = "on-failure";
          RestartSec = 30;
        };
      };

      services.openssh = {
        enable = lib.mkDefault (repoArgs.operatorSshKey != null);
        settings.PasswordAuthentication = false;
      };

      ###### claude-session-publisher
      # On boot, watch the `claude remote-control` typescript for the
      # session URL that the CLI prints on registration (a claude.ai link
      # the operator drives the agent from), and POST it to the admin
      # service's internal endpoint on the cache-bridge gateway.
      #
      # The admin service identifies us by source IP against the vm_ip
      # column — no token needed. Once a URL is published, the unit
      # exits successfully and stays out of the way.
      #
      # The typescript is full of terminal escapes; `strings` strips
      # non-printable bytes so the URL falls out cleanly. The regex is
      # deliberately broad (any https://claude.ai/... URL) because the
      # exact session-URL format from `claude remote-control` isn't
      # stable across releases — we just want the first one we see.
      systemd.services.claude-session-publisher = {
        description = "Publish claude remote-control session URL to admin";
        after = [ "claude-remote.service" ];
        bindsTo = [ "claude-remote.service" ];
        wantedBy = [ "multi-user.target" ];
        path = with pkgs; [ coreutils binutils curl gnugrep gawk ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          # Give the agent a moment to register the session before we
          # start tailing the typescript.
          ExecStart = pkgs.writeShellScript "claude-session-publisher" ''
            set -euo pipefail
            TYPESCRIPT=/tmp/claude-remote.typescript
            ADMIN_URL="http://10.42.0.1:8444/v1/internal/session-url"

            # Wait up to ~5 minutes for the file to appear and grow.
            for _ in $(seq 1 60); do
              [ -s "$TYPESCRIPT" ] && break
              sleep 5
            done

            # Poll the typescript for a claude.ai URL. The session
            # registration line shows up within seconds of `claude
            # remote-control` printing its banner; if it hasn't after
            # ~5 minutes, something is wrong and we exit nonzero so the
            # journal records the failure.
            URL=""
            for _ in $(seq 1 60); do
              if [ -s "$TYPESCRIPT" ]; then
                URL="$(${pkgs.binutils}/bin/strings "$TYPESCRIPT" \
                  | ${pkgs.gnugrep}/bin/grep -oE 'https://claude\.ai/[A-Za-z0-9./?=&%_~+#-]+' \
                  | ${pkgs.coreutils}/bin/head -n 1 || true)"
              fi
              if [ -n "$URL" ]; then break; fi
              sleep 5
            done

            if [ -z "$URL" ]; then
              echo "no claude.ai session URL found in $TYPESCRIPT after 5min" >&2
              exit 1
            fi

            echo "publishing session URL: $URL"
            # 5s connect timeout, 10s total, retry a couple times in case
            # the admin service is mid-restart.
            ${pkgs.curl}/bin/curl --fail --silent --show-error \
              --connect-timeout 5 --max-time 10 \
              --retry 3 --retry-delay 5 \
              -H 'Content-Type: application/json' \
              -d "{\"url\":\"$URL\"}" \
              "$ADMIN_URL"
          '';
          Restart = "on-failure";
          RestartSec = 30;
          # If the URL never gets posted (e.g. typescript empty), we don't
          # want the unit retrying forever and spamming the journal.
          StartLimitBurst = 3;
          StartLimitIntervalSec = 600;
        };
      };

      ###### Resource accounting — give a second OOM fence inside the VM.
      systemd.slices."claude.slice".sliceConfig = {
        MemoryHigh = "${toString (repoArgs.memMb * 80 / 100)}M";
      };
      systemd.services.claude-remote.serviceConfig.Slice = "claude.slice";
    })
  ];
}
