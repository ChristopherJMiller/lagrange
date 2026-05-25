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
#
# This file is the microvm/network/cache wrapper. All the claude-related
# guest config (systemd services, tmpfiles, fileSystems, the agent user)
# lives in ./repo-vm-guest.nix so tests/repo-vm-boot.nix can boot the
# same units under a regular nixos test VM without microvm wrapping.

{ name
, repoUrl
, branch ? "main"
, vmIp
, vmMac
, vcpu ? 2
, memMb ? 8192
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
  # Where the guest's claude-session-publisher POSTs the scraped URL.
  # Default is the cache-bridge gateway, where the admin service's
  # internal listener accepts source-IP-identified callbacks. The test
  # overrides this to point at a local stub.
, adminCallbackUrl ? "http://10.42.0.1:8444/v1/internal/session-url"
}:

nixpkgs.lib.nixosSystem {
  inherit system;

  specialArgs = {
    inherit microvm claude-code;
    repoArgs = {
      inherit name repoUrl branch vmIp vmMac vcpu memMb
        operatorSshKey permissionMode adminCallbackUrl;
    };
  };

  modules = [
    microvm.nixosModules.microvm

    # Shared guest-claude config (units, mounts, agent user).
    ./repo-vm-guest.nix

    # Microvm/network/cache wrapping, only meaningful when actually
    # running as a microvm under cloud-hypervisor.
    ({ config, pkgs, lib, repoArgs, ... }: {
      system.stateVersion = "25.11";

      ###### Toolchain (claude-code overlay + the unfree license predicate
      # it requires). Moved here from the shared guest module so the
      # repo-vm-boot test — which overrides nixpkgs via runNixOSTest's
      # read-only nixpkgs.config — can choose its own approach.
      nixpkgs.overlays = [ claude-code.overlays.default ];
      nixpkgs.config.allowUnfreePredicate = pkg:
        builtins.elem (lib.getName pkg) [ "claude-code" ];

      ###### microVM configuration
      microvm = {
        hypervisor = "cloud-hypervisor";
        vcpu = repoArgs.vcpu;
        mem = repoArgs.memMb;
        balloon = true;

        # Mount the virtiofs host /nix/store at /nix/.ro-store (read-only)
        # and overlay /nix/.rw-store at /nix/store. Without an overlay,
        # every write to /nix/store hits the read-only virtiofs mount
        # and fails with EROFS — `nix-shell`, `nix develop`, `nix build`,
        # and anything that downloads from a substituter all break.
        writableStoreOverlay = "/nix/.rw-store";

        # Back the writable overlay with a per-VM sparse disk image
        # instead of letting it land on the rootfs tmpfs (microvm.nix's
        # default, 50% of guest RAM). The first agent that ran a
        # nontrivial `nix develop` here filled the RAM-backed overlay
        # immediately — a Rust devShell closure is multi-GB, and the
        # tier sizes we run (2–32 GiB RAM) would all run out before the
        # devShell finished. ext4 on a 32 GiB sparse raw image gives
        # the agent room without depending on RAM at all; sparse means
        # actual host-disk usage is only what's written. Cleaned up by
        # `rm -rf /var/lib/microvms/<name>/` on `microvm -d` (the admin
        # service's rm-rf fallback handles this).
        volumes = [{
          image = "/var/lib/microvms/${repoArgs.name}/nix-overlay.img";
          mountPoint = "/nix/.rw-store";
          size = 32768;
          fsType = "ext4";
        }];

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

      ###### Package-manager cache routing
      environment.variables = {
        CARGO_NET_GIT_FETCH_WITH_CLI = "true";
        npm_config_registry = "http://cache.internal:4873/";
        GOPROXY = "http://cache.internal:3000,direct";
        UV_INDEX_URL = "http://cache.internal:3141/root/pypi/+simple/";
        PIP_INDEX_URL = "http://cache.internal:3141/root/pypi/+simple/";
      };

      # NOTE: the lagrange attic cache is intentionally NOT wired in here.
      # atticd requires a bearer token per pull and we don't have a
      # readable-without-auth mode set up yet — the previous attempt
      # left a placeholder public key in `trusted-public-keys` and
      # caused every `nix-shell` invocation to fail with HTTP 401 when
      # nix tried the substituter first. Until the cache is either
      # opened up or we plumb a token into the guest, fall back to
      # nixpkgs' default substituters (cache.nixos.org), which is
      # sufficient for everything in the standard library.

      # Cargo config: route through nginx HTTP cache on cache.internal:7878.
      environment.etc."skel/.cargo/config.toml".text = ''
        [source.crates-io]
        replace-with = "lagrange"

        [source.lagrange]
        registry = "sparse+http://cache.internal:7878/index/"
      '';
    })
  ];
}
