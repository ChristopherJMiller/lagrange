{ self, nixpkgs, pkgs }:

# End-to-end smoke test for the lagrange-admin service.
#
# Boots a single NixOS VM with the admin module active, plaintext bearer token
# in place of sops, listens on 127.0.0.1 instead of the WG IP. Confirms:
#   - service comes up
#   - /v1/health returns 200 with the bearer
#   - /v1/health returns 401 without the bearer
#   - /v1/repos (GET) returns an empty list
#
# This is the broadest single test of the whole module-to-binary wiring.

let
  testToken = "lagrange-test-token-please-change";
in
pkgs.testers.runNixOSTest {
  name = "lagrange-admin-smoke";

  nodes.machine = { config, pkgs, lib, ... }: {
    imports = [
      self.nixosModules.lagrangeAdmin
      ./lib.nix
    ];

    lagrange.admin = {
      enable = true;
      bindAddress = "127.0.0.1";
      bindPort = 8443;
      requireWireguard = false;
      requireSops = false;
      tokenFile = pkgs.writeText "admin-test-token" testToken;
      deployKeysTarFile = null;
      # Test runs against 127.0.0.1, so the production SSO-peer trust IP
      # would never match anyway — but turn the path off explicitly so the
      # smoke test exercises only the bearer-token policy.
      trustedSsoPeer = null;
      logLevel = "debug";
    };

    environment.systemPackages = with pkgs; [ curl jq sqlite ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("lagrange-admin.service")
    machine.wait_for_open_port(8443)

    with subtest("missing bearer is rejected"):
        rc, _ = machine.execute(
            "curl -fsS -o /dev/null http://127.0.0.1:8443/v1/health"
        )
        assert rc != 0, "request without Authorization should not return 2xx"

    with subtest("wrong bearer is rejected"):
        rc, _ = machine.execute(
            "curl -fsS -o /dev/null -H 'Authorization: Bearer wrong' "
            "http://127.0.0.1:8443/v1/health"
        )
        assert rc != 0, "request with wrong bearer should not return 2xx"

    with subtest("/v1/health returns ok with correct bearer"):
        out = machine.succeed(
            "curl -fsS -H 'Authorization: Bearer ${testToken}' "
            "http://127.0.0.1:8443/v1/health"
        )
        machine.succeed(
            f"printf '%s' '{out.strip()}' | jq -e '.status == \"ok\"' "
        )
        machine.succeed(
            f"printf '%s' '{out.strip()}' | jq -e '.vms_total == 0' "
        )

    with subtest("/v1/repos returns empty list"):
        out = machine.succeed(
            "curl -fsS -H 'Authorization: Bearer ${testToken}' "
            "http://127.0.0.1:8443/v1/repos"
        )
        machine.succeed(
            f"printf '%s' '{out.strip()}' | jq -e 'length == 0'"
        )

    with subtest("sqlite schema is on disk"):
        machine.succeed("test -f /var/lib/lagrange-admin/state.db")
        machine.succeed(
            "sqlite3 /var/lib/lagrange-admin/state.db "
            "'SELECT count(*) FROM repo_vms'"
        )
        # IP pool is seeded with 190 addresses (.10 through .199).
        out = machine.succeed(
            "sqlite3 /var/lib/lagrange-admin/state.db "
            "'SELECT count(*) FROM ip_pool'"
        )
        assert out.strip() == "190", f"unexpected ip_pool size {out!r}"
  '';
}
