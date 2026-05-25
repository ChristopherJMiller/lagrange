{ pkgs, lib, repoArgs, ... }:

# NOTE: do NOT add `claude-code` to the function args. It's tempting
# because we used to set the overlay here, but as a lexical binding it
# shadows `pkgs.claude-code` inside `with pkgs;` blocks below — and
# evaluation then tries to use the flake input (an attrset) as a
# package, which fails with a confusing "not of type `package`" error.
# The overlay lives in lib/repo-vm.nix's wrapper now.

# Guest-side claude bits of a repo-VM — extracted from lib/repo-vm.nix
# so the e2e test (tests/repo-vm-boot.nix) and the real mkRepoVm both
# consume the same systemd-unit / fileSystems / tmpfiles definitions.
# Drift between them would defeat the test.
#
# Contains everything the agent needs to actually run inside the guest:
# the claude-code overlay, system packages, the `agent` user, the
# persistent-volume mount layout, and the two services (claude-remote
# and claude-session-publisher).
#
# Does NOT contain microvm.{shares,interfaces,vcpu,mem}, network
# interface config (IP/MAC/gateway), or cache-routing env vars — those
# live in lib/repo-vm.nix because they're tied to running as a
# microvm under cloud-hypervisor on a real satellite. The test
# substitutes its own minimal scaffolding for them.

let
  # `--dangerously-skip-permissions` is a TOP-LEVEL claude flag (before
  # the subcommand); `--permission-mode` is a remote-control subcommand
  # flag. The DB CHECK in the matching admin migration mirrors this enum.
  claudeInvocation =
    if repoArgs.permissionMode == "dangerously-skip" then
      "${pkgs.claude-code}/bin/claude --dangerously-skip-permissions remote-control --name ${repoArgs.name} --spawn same-dir --verbose"
    else
      "${pkgs.claude-code}/bin/claude remote-control --name ${repoArgs.name} --spawn same-dir --permission-mode auto --verbose";
in
{
  ###### Toolchain
  # nixpkgs.overlays + nixpkgs.config.allowUnfreePredicate live in the
  # mkRepoVm wrapper (lib/repo-vm.nix), not here, because the test
  # (tests/repo-vm-boot.nix) uses runNixOSTest which marks
  # nixpkgs.config read-only and provides its own overlay path. Both
  # consumers wire claude-code into `pkgs` before this module evaluates,
  # so the `pkgs.claude-code` references below resolve regardless.

  # microvm.nixosModules.microvm masks nix-daemon by default to keep
  # guest closures small — microvms aren't expected to evaluate nix.
  # Our guest IS a dev shell for an interactive agent that runs
  # `nix develop`, `nix flake show`, etc., so unmask. The microvm
  # wrapper layers a writableStoreOverlay over the virtiofs-mounted
  # /nix/.ro-store so the guest CAN realise new derivations (writes
  # land in the tmpfs overlay; the host store stays read-only).
  nix.enable = lib.mkForce true;

  # Without these, every modern nix invocation prints
  # "error: experimental Nix feature 'nix-command' is disabled".
  # The agent reaches for `nix develop`, `nix shell`, `nix flake`,
  # and `nix-shell -p` constantly — all of those need the flag.
  nix.settings.experimental-features = [ "nix-command" "flakes" ];

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
    # Common dev tools agents tend to reach for. Curated set — the
    # agent can `nix shell nixpkgs#<pkg>` for anything else once nix
    # is enabled (see `nix.enable` above).
    sqlite     # quick DB inspection / migration testing
    gnumake    # most Rust/C projects expect `make`
    gcc        # native compilation (proc macros, build.rs, FFI)
    pkg-config # discovers system libs for native crates
    curl       # already available via path on claude-remote, but
               # also drop into PATH for interactive shell sessions
    openssl    # cert poking, JWT inspection, etc.
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

    # Pre-create bind-mount target DIRS. systemd's fstab-generated
    # mount units don't auto-create their targets — production happened
    # to work because the dirs already existed from a prior boot, but
    # a clean boot needs these explicit. Same pattern as the existing
    # f-rules for credentials.json / claude.json below (file targets
    # for single-file binds).
    "d /home/agent/work               0755 agent users -"
    "d /home/agent/.claude/projects   0755 agent users -"
    "d /home/agent/.claude/todos      0755 agent users -"
    "d /home/agent/.claude/statsig    0755 agent users -"
    "d /home/agent/.ssh               0700 agent users -"

    # NOTE: NO tmpfiles `f` rules for ~/.claude/.credentials.json or
    # ~/.claude.json. They used to be bind-mount targets, but that
    # made claude's oauth-refresh writes fail with EBUSY (atomic
    # rename refused on bind-mounted single files) and the access
    # token would silently expire every ~24h. Replaced with an
    # ExecStartPre copy from /persistent/* and a periodic sync back
    # (claude-credentials-sync.timer below).

    "d /persistent/projects 0755 agent users -"
    "d /persistent/todos    0755 agent users -"
    "d /persistent/statsig  0755 agent users -"
    "d /persistent/ssh      0700 agent users -"
    "d /persistent/work     0755 agent users -"
    # gitconfig lives in its own agent-owned dir, not at /persistent/
    # root. `git config --global` creates a `.lock` file in the SAME
    # DIRECTORY as the config target — /persistent/ itself is owned
    # by lagrange-admin on the host so agent inside the guest can't
    # mkfile there. Inside a subdir owned by agent it works fine.
    "d /persistent/git      0755 agent users -"
    "f /persistent/git/config 0644 agent users -"
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
      # we set GIT_CONFIG_GLOBAL=/persistent/git/config in the
      # claude-remote service Environment so git treats the persistent
      # file as "global" and writes to it directly — rename works
      # because there's no mount in the way.
      #
      # Same reason for .credentials.json / .claude.json: claude does
      # atomic-rename when it refreshes the oauth token, which fails
      # on bind-mounted single files. Those files live as real
      # writable files in /home/agent now, seeded from /persistent
      # by ExecStartPre on claude-remote and synced back by
      # claude-credentials-sync.timer.
    };

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
        # Persist git config to /persistent/git/config directly
        # instead of bind-mounting ~/.gitconfig (which broke the
        # atomic-rename `git config --global` uses) — and inside its
        # own subdir because git creates `<config>.lock` in the same
        # dir, and /persistent/ root is lagrange-admin-owned (so the
        # agent user can't make files there). The subdir is
        # agent:users 0755 via the tmpfiles rule above.
        "GIT_CONFIG_GLOBAL=/persistent/git/config"
        # Wire a usable PATH for shells the agent spawns via claude's
        # Bash tool. Without this, the service inherits systemd's
        # minimal default ($PATH = /usr/local/sbin:/usr/local/bin:
        # /usr/sbin:/usr/bin:/sbin:/bin) — none of those exist on
        # NixOS, so `nix`, `nix-shell`, `sudo`, and even `ls` come
        # back as "command not found" from inside the agent. Order
        # matters: wrappers first (for setuid sudo), then the system
        # profile, then the user profiles.
        "PATH=/run/wrappers/bin:/run/current-system/sw/bin:/home/agent/.nix-profile/bin:/etc/profiles/per-user/agent/bin"
      ];
      # Optional env files staged by the admin service. Leading `-`
      # makes each file optional so VMs whose corresponding host-side
      # secret hasn't been POSTed yet still boot:
      #   agent.env  — CLAUDE_CODE_OAUTH_TOKEN (inference-only token)
      #   gh.env     — GITHUB_TOKEN / GH_TOKEN for `git push`
      EnvironmentFile = [ "-/persistent/agent.env" "-/persistent/gh.env" ];

      # Seed (or refresh from) /persistent's authoritative copy of
      # the operator's claude session. We don't bind-mount these
      # single files because claude does atomic-rename when it
      # refreshes the oauth access token, which fails with EBUSY on
      # a bind-mount. Instead: copy in here, let claude refresh
      # in-place, claude-credentials-sync.timer copies the refreshed
      # bundle back to /persistent so the next restart and the next
      # `microvm -d` + redeploy both inherit the fresh token. The
      # operator's stage_for_vm is authoritative on restart — copy
      # is unconditional.
      ExecStartPre = pkgs.writeShellScript "claude-remote-prestart" ''
        set -e
        mkdir -p /home/agent/.claude
        if [ -s /persistent/credentials.json ]; then
          cat /persistent/credentials.json > /home/agent/.claude/.credentials.json
          chmod 0600 /home/agent/.claude/.credentials.json
        fi
        if [ -s /persistent/claude.json ]; then
          cat /persistent/claude.json > /home/agent/.claude.json
          chmod 0600 /home/agent/.claude.json
        fi
      '';
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
          # `git config` without --add OVERWRITES the value, so setting
          # url.<>.insteadOf twice loses the first one. We need both
          # rewrites (git@github.com: AND ssh://git@github.com/) to
          # cover both URL forms the operator might paste. Use
          # --replace-all once to start clean (idempotent across
          # service restarts) then --add the rest.
          ${pkgs.git}/bin/git config --global --replace-all \
            url.https://github.com/.insteadOf "git@github.com:"
          ${pkgs.git}/bin/git config --global --add \
            url.https://github.com/.insteadOf "ssh://git@github.com/"
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

        # script(1) captures the TUI (full of terminal escapes) to a
        # typescript file so an operator who ssh's in can read the
        # session URL/QR code claude prints on startup. The systemd
        # journal only sees `[NNB blob data]` lines for the same
        # output, which isn't useful.
        #
        # `-f` (--flush) writes each line to the typescript as it
        # arrives. Without this, script buffers output and the small
        # initial burst (welcome banner + session URL) sits unflushed
        # until either the buffer fills or the child exits — neither
        # happens for `claude remote-control` in its quiet TUI steady
        # state, so the publisher's 5min poll on /tmp/claude-remote.
        # typescript times out without ever seeing the URL. Caught by
        # the repo-vm-boot e2e test.
        exec ${pkgs.util-linux}/bin/script -qf \
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

  ###### claude-credentials-sync
  # Claude refreshes its oauth access token every ~24h (the bundle
  # has a refreshToken with a longer life). On the laptop that
  # refresh writes back to ~/.claude/.credentials.json and the cycle
  # continues indefinitely. In the guest we used to bind-mount that
  # file, which made the atomic rename fail and the refresh
  # invisibly drop on the floor — so after one access-token cycle
  # the agent would crashloop with 401 until the operator re-staged.
  #
  # New design: the file lives in /home/agent/.claude as a real
  # writable file (ExecStartPre seeds it from /persistent on each
  # claude-remote start). This timer + oneshot syncs the refreshed
  # bundle BACK to /persistent on a 5min cadence so it survives
  # restarts and destroy+redeploy. The newness check means an
  # operator restage via orbit isn't clobbered between sync ticks.
  systemd.services.claude-credentials-sync = {
    description = "Persist refreshed claude credentials back to /persistent";
    serviceConfig = {
      Type = "oneshot";
      User = "agent";
      ExecStart = pkgs.writeShellScript "claude-credentials-sync" ''
        set -e
        # Direct overwrite (cat > file) not cp, because /persistent/
        # is lagrange-admin-owned on the host and agent can't create
        # the .tmp companion `cp` would otherwise use. Writing to the
        # existing inode is allowed (the file itself is agent-owned).
        sync_if_newer() {
          src="$1"; dst="$2"
          if [ -s "$src" ] && [ "$src" -nt "$dst" ]; then
            cat "$src" > "$dst"
          fi
        }
        sync_if_newer /home/agent/.claude/.credentials.json /persistent/credentials.json
        sync_if_newer /home/agent/.claude.json              /persistent/claude.json
      '';
    };
  };
  systemd.timers.claude-credentials-sync = {
    description = "Periodic sync of refreshed claude credentials";
    wantedBy = [ "timers.target" ];
    timerConfig = {
      # Give claude-remote a minute to seed + actually start before
      # we look for a refresh, then check every 5 min. Claude
      # typically refreshes hours before expiry, so 5min granularity
      # leaves plenty of headroom.
      OnBootSec = "1min";
      OnUnitActiveSec = "5min";
    };
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
    # StartLimit* options live on the unit, not the service — systemd
    # silently ignores them in [Service]. Catching this from the test
    # was the giveaway: the warning showed up in the guest journal.
    unitConfig = {
      StartLimitBurst = 3;
      StartLimitIntervalSec = 600;
    };
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      ExecStart = pkgs.writeShellScript "claude-session-publisher" ''
        set -euo pipefail
        TYPESCRIPT=/tmp/claude-remote.typescript
        ADMIN_URL="${repoArgs.adminCallbackUrl or "http://10.42.0.1:8444/v1/internal/session-url"}"

        # Wait up to ~5 minutes for the file to appear and grow.
        for _ in $(seq 1 60); do
          [ -s "$TYPESCRIPT" ] && break
          sleep 5
        done

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
        ${pkgs.curl}/bin/curl --fail --silent --show-error \
          --connect-timeout 5 --max-time 10 \
          --retry 3 --retry-delay 5 \
          -H 'Content-Type: application/json' \
          -d "{\"url\":\"$URL\"}" \
          "$ADMIN_URL"
      '';
      Restart = "on-failure";
      RestartSec = 30;
    };
  };

  ###### Resource accounting — give a second OOM fence inside the VM.
  systemd.slices."claude.slice".sliceConfig = {
    MemoryHigh = "${toString (repoArgs.memMb * 80 / 100)}M";
  };
  systemd.services.claude-remote.serviceConfig.Slice = "claude.slice";
}
