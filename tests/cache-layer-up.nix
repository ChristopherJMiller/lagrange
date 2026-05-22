{ self, nixpkgs, pkgs }:

# Boots a NixOS VM with the cache layer module active and confirms each
# daemon comes up and listens on a port we can reach from inside the VM.
#
# Verdaccio is excluded — it runs as an OCI container that pulls an image
# from docker.io, which is not available in the sandboxed test environment.
# Atticd is excluded because its env file (the RS256 secret) is sops-managed.
# A future test can synthesize a fake env file and add atticd back.

pkgs.testers.runNixOSTest {
  name = "lagrange-cache-layer-up";

  nodes.machine = { config, pkgs, lib, ... }: {
    imports = [
      ../modules/cache-layer.nix
      ./lib.nix
    ];

    # Run the cache daemons on 127.0.0.1 inside the test machine — loopback
    # is always up, so we sidestep the secondary-address timing issue that
    # broke binding to 10.42.0.1 in earlier runs.
    lagrange.cacheLayer.bridgeIp = "127.0.0.1";

    # Disable services that depend on resources the test sandbox can't
    # provide:
    # - atticd needs an RS256 env file we don't synthesize
    # - verdaccio runs as an OCI container (would pull from docker.io)
    # - docker-registry in proxy mode panics if it can't DNS-resolve
    #   registry-1.docker.io at startup, and the test VM has no
    #   internet/DNS
    services.atticd.enable = lib.mkForce false;
    services.dockerRegistry.enable = lib.mkForce false;
    virtualisation.oci-containers.containers = lib.mkForce { };
    virtualisation.podman.enable = lib.mkForce false;

    environment.systemPackages = with pkgs; [ netcat-openbsd curl ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("athens (go module proxy) listens"):
        machine.wait_for_unit("athens.service")
        machine.wait_until_succeeds("nc -z 127.0.0.1 3000", timeout=30)

    with subtest("devpi (pypi proxy) listens"):
        machine.wait_for_unit("devpi-server.service")
        machine.wait_until_succeeds("nc -z 127.0.0.1 3141", timeout=30)

    with subtest("nginx cargo cache listens"):
        machine.wait_for_unit("nginx.service")
        machine.wait_until_succeeds("nc -z 127.0.0.1 7878", timeout=30)
  '';
}
