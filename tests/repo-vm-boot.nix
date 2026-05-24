{ self, nixpkgs, pkgs, claude-code }:

# End-to-end test of the GUEST side of mkRepoVm.
#
# Boots a regular NixOS test VM that imports lib/repo-vm-guest.nix (the
# same module mkRepoVm composes for the real microvm) and asserts the
# things we've seen go wrong in production:
#
#   1. claude-remote.service starts cleanly and stays active for
#      > 30s — catches `gh auth setup-git` PATH failures, gitconfig
#      EBUSY on bind-mounted files, gitconfig .lock permission denied,
#      and any future ExecStart regression that exits non-zero.
#   2. /persistent/git/config gets the BOTH insteadOf values (git@:
#      AND ssh://). Catches the --replace-all/--add mistake where
#      setting the same key twice silently overwrites the first
#      value, leaving git@github.com: URLs unrewritten.
#   3. claude-session-publisher.service isn't dragged down by
#      `bindsTo=claude-remote.service` — if it shows up as `failed`
#      with TERM right after start, claude-remote must have died.
#
# The test STUBS claude-code (the real package isn't free-redistributable
# and we don't want a real network call) and SKIPS the git clone by
# pre-populating /persistent/work/.git. The publisher's HTTPS callback
# is pointed at a tiny localhost stub so it doesn't fail on network.

let
  # Fake claude-code: prints a session URL (so the publisher's regex
  # finds something to scrape) and sleeps. systemd's `script` wrapper
  # stays alive as long as its child does, so claude-remote.service
  # stays "active" indefinitely — which is what the production unit
  # is supposed to do.
  fakeClaudeCode = pkgs.symlinkJoin {
    name = "claude-code-stub";
    paths = [
      (pkgs.writeShellScriptBin "claude" ''
        # Mimic the prefix of real claude output so the publisher's
        # `grep -oE 'https://claude\.ai/...'` matches. Use stdbuf so
        # the line is flushed immediately — under `script -q`, small
        # bursts of output that the child writes then exits/sleeps on
        # were not making it into the typescript file on time.
        ${pkgs.coreutils}/bin/stdbuf -oL -eL echo "Welcome to Claude Code"
        ${pkgs.coreutils}/bin/stdbuf -oL -eL echo "Drive the session from https://claude.ai/code/sessions/stub-12345"
        # remote-control mode is the long-running server in production.
        # Print a heartbeat so script(1) keeps flushing the typescript
        # — without continuous output, the burst above sometimes sat
        # in the pty buffer past the publisher's poll window.
        if [ "$1" = "remote-control" ] || [ "$2" = "remote-control" ]; then
          while true; do
            sleep 5
            ${pkgs.coreutils}/bin/stdbuf -oL echo "heartbeat"
          done
        fi
        exit 0
      '')
    ];
    # Match the shape of the real claude-code attr enough that
    # `${pkgs.claude-code}/bin/claude` resolves the same way.
    meta = { mainProgram = "claude"; };
  };

  # Overlay that swaps the real claude-code for the stub. lib/repo-vm-guest.nix
  # asks for it via `claude-code.overlays.default` AND `pkgs.claude-code`;
  # we monkey-patch both so the systemd unit's `${pkgs.claude-code}/bin/claude`
  # interpolation in the ExecStart resolves to the stub.
  stubClaudeCodeFlake = {
    overlays.default = final: prev: { claude-code = fakeClaudeCode; };
  };

  # runNixOSTest's nodes have nixpkgs.{config,overlays} locked as
  # read-only. The supported way to inject a custom claude-code is to
  # call testers.runNixOSTest on a pkgs that already has the overlay
  # applied — then the locked overlays already include our stub.
  pkgsWithStub = pkgs.extend stubClaudeCodeFlake.overlays.default;

  testToken = "ghp_fake_test_token_used_offline";
in
pkgsWithStub.testers.runNixOSTest {
  name = "lagrange-repo-vm-boot";

  nodes.guest = { config, pkgs, lib, ... }: {
    imports = [
      ./lib.nix

      # Same module the real mkRepoVm uses — but the pkgs it sees has
      # claude-code overlaid to the stub above.
      (import ../lib/repo-vm-guest.nix)
    ];

    _module.args = {
      repoArgs = {
        name = "boottest";
        repoUrl = "git@github.com:fake/boottest.git";
        branch = "main";
        vmIp = "10.0.2.15";
        vmMac = "02:00:00:00:00:01";
        vcpu = 1;
        memMb = 1024;
        operatorSshKey = null; # no ssh in the test
        permissionMode = "auto";
        # Publisher callback points at a localhost stub (set up below).
        adminCallbackUrl = "http://127.0.0.1:18444/v1/internal/session-url";
      };
    };

    # Replace the /home/agent/work bind-mount with a plain tmpfs in
    # the test. We don't actually need to verify "the bind-mount
    # works" (the real microvm sources /persistent from a virtiofs
    # share, which would be a different test) — what we DO need is
    # for tmpfiles to be able to pre-create a .git marker inside
    # /home/agent/work so the start script's `[ ! -d .git ]` clone
    # branch is skipped (no internet in the test sandbox). On a
    # tmpfs, tmpfiles is guaranteed to write after the mount.
    fileSystems."/home/agent/work" = lib.mkForce {
      device = "tmpfs";
      fsType = "tmpfs";
      options = [ "uid=1000" "gid=100" "mode=0755" ];
    };

    # Pre-stage everything the start script reads:
    #   - /home/agent/work/.git so `if [ ! -d .git ]; then clone` is skipped
    #   - /persistent/gh.env so the GITHUB_TOKEN-conditional branch in
    #     the start script is exercised — that's where the gh auth
    #     setup-git + insteadOf rewrites live, which is what we're
    #     here to test
    #   - fake credentials.json / claude.json so the bind-mounts have
    #     valid targets
    systemd.tmpfiles.rules = [
      "d  /home/agent/work/.git    0755 agent users -"
      "f+ /persistent/gh.env       0640 agent users - GITHUB_TOKEN=${testToken}\\nGH_TOKEN=${testToken}\\n"
      "f+ /persistent/credentials.json 0640 agent users - {\"fake\":\"creds\"}"
      "f+ /persistent/claude.json      0640 agent users - {\"fake\":\"claude\"}"
      # The real guest mounts /shared from the host's agent-shared
      # directory. The test fakes those targets so the symlinks in
      # systemd.tmpfiles (CLAUDE.md, skills, commands) resolve.
      "d  /shared                  0755 root  root  -"
      "f  /shared/CLAUDE.md        0644 root  root  -"
      "d  /shared/skills           0755 root  root  -"
      "d  /shared/commands         0755 root  root  -"
    ];

    # Tiny stub admin endpoint so the publisher's POST gets a 204
    # instead of hanging on the cache-bridge gateway that doesn't
    # exist here. Logs every request to a file the test can inspect.
    systemd.services.fake-admin = {
      description = "Stub admin internal endpoint for repo-vm-boot test";
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = pkgs.writeShellScript "fake-admin" ''
        # Accept any POST, log the body, respond 204.
        ${pkgs.python3}/bin/python3 -c '
        import http.server, json, sys
        class H(http.server.BaseHTTPRequestHandler):
          def do_POST(self):
            n = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(n).decode()
            with open("/tmp/admin-callbacks.log", "a") as f:
              f.write(f"{self.path} {body}\n")
            self.send_response(204); self.end_headers()
          def log_message(self, *a): pass
        http.server.HTTPServer(("127.0.0.1", 18444), H).serve_forever()
        '
      '';
    };
  };

  testScript = ''
    guest.wait_for_unit("multi-user.target")
    guest.wait_for_unit("fake-admin.service")

    # Diagnostics that have already saved us a round-trip when something
    # in the bind-mount or tmpfiles setup drifts.
    print(guest.succeed("ls -la /persistent /persistent/work /home/agent /home/agent/work 2>&1 || true"))

    # Bug-catcher #1: claude-remote must reach 'active' and stay there.
    # The PATH and gitconfig bugs both caused this to crashloop within
    # the first second.
    guest.wait_until_succeeds(
        "test \"$(systemctl is-active claude-remote.service)\" = active",
        timeout=60,
    )
    # 30s settle window — if a crashloop is hidden behind RestartSec,
    # this will catch it.
    import time
    time.sleep(30)
    state = guest.succeed("systemctl is-active claude-remote.service").strip()
    assert state == "active", f"claude-remote went non-active: {state}\n" + \
        guest.succeed("journalctl -u claude-remote -n 50 --no-pager")

    # Bug-catcher #2: BOTH insteadOf values are present. Catches the
    # `git config` overwrite-vs-add mistake where setting the same key
    # twice silently dropped the first value.
    insteadof = guest.succeed(
        "sudo -u agent GIT_CONFIG_GLOBAL=/persistent/git/config "
        "git config --get-all url.https://github.com/.insteadOf"
    ).strip().splitlines()
    assert "git@github.com:" in insteadof, f"missing git@ rewrite: {insteadof!r}"
    assert "ssh://git@github.com/" in insteadof, f"missing ssh:// rewrite: {insteadof!r}"

    # Bug-catcher #3: publisher should successfully POST to the stub
    # and the admin endpoint should have logged a session URL.
    # Diagnose typescript state in case the publisher fails to find a URL.
    print("typescript:", guest.succeed("ls -la /tmp/claude-remote.typescript 2>&1 || echo MISSING"))
    print("typescript strings (first 30):",
          guest.succeed("strings /tmp/claude-remote.typescript 2>&1 | head -30 || echo EMPTY"))

    guest.wait_until_succeeds(
        "test -s /tmp/admin-callbacks.log",
        timeout=120,
    )
    callback_log = guest.succeed("cat /tmp/admin-callbacks.log")
    assert "/v1/internal/session-url" in callback_log, f"unexpected callback: {callback_log!r}"
    assert "claude.ai" in callback_log, f"no claude.ai URL in callback body: {callback_log!r}"

    # Bug-catcher #4: publisher should not be in 'failed' state. If
    # claude-remote ever died, bindsTo would have killed the publisher
    # with SIGTERM — distinct from the normal "ran once, exit 0,
    # RemainAfterExit=true → active".
    pub_state = guest.succeed("systemctl is-active claude-session-publisher.service").strip()
    assert pub_state == "active", f"publisher went non-active: {pub_state}\n" + \
        guest.succeed("journalctl -u claude-session-publisher -n 30 --no-pager")
  '';
}
