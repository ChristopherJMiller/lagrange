{ config, lib, pkgs, ... }:

# Host-side package-manager caching layer. Daemons bind to the cache bridge IP
# (default 10.42.0.1) and are reachable from repo-VMs as cache.internal (via
# dnsmasq in hosts/lagrange/networking.nix). Storage lives under
# /var/lib/cache/<daemon>/.

let
  cfg = config.lagrange.cacheLayer;
  bridgeIp = cfg.bridgeIp;
  cargoProxyPort = 7878;
in
{
  options.lagrange.cacheLayer = {
    bridgeIp = lib.mkOption {
      type = lib.types.str;
      default = "10.42.0.1";
      description = ''
        Bind address for cache daemons. Production uses the cache bridge IP;
        tests can set this to 127.0.0.1 to avoid relying on a secondary
        interface address being ready by the time services start.
      '';
    };
  };

  config = {
  ###### attic — Nix binary cache (pull-through)
  # The atticd nixos module uses systemd DynamicUser, so it normally would
  # not create an entry in /etc/passwd. sops-nix runs `user.Lookup("atticd")`
  # during activation before any service starts, and fails the whole secret
  # setup if the user is missing. Declare the user/group statically so the
  # lookup succeeds; systemd's DynamicUser will reuse the static UID.
  users.users.atticd = {
    isSystemUser = true;
    group = "atticd";
  };
  users.groups.atticd = { };

  services.atticd = {
    enable = true;
    environmentFile = config.sops.secrets.atticd-env.path;

    settings = {
      listen = "${bridgeIp}:8080";
      api-endpoint = "http://cache.internal:8080/";

      # Sqlite file lives inside storage/ because that's the only writable
      # path under the hardened atticd unit (ProtectSystem=strict +
      # ReadWritePaths=/var/lib/cache/atticd/storage). Trying to open the
      # DB anywhere else in /var/lib/cache/atticd fails with EROFS.
      database.url = "sqlite:///var/lib/cache/atticd/storage/server.db?mode=rwc";

      storage = {
        type = "local";
        path = "/var/lib/cache/atticd/storage";
      };

      chunking = {
        nar-size-threshold = 65536;
        min-size = 16384;
        avg-size = 65536;
        max-size = 262144;
      };

      garbage-collection = {
        interval = "12 hours";
        default-retention-period = "30 days";
      };
    };
  };

  ###### Athens — Go module proxy
  services.athens = {
    enable = true;
    port = 3000;
    storageType = "disk";
    storage.disk.rootPath = "/var/lib/cache/athens";
    downloadMode = "async_redirect";
    networkMode = "strict";
    logLevel = "warning";
  };

  # Athens binds to all interfaces by default; the bridge is the only reachable
  # path because nftables drops Athens's port on every other interface. Keeping
  # the upstream module's hardening (DynamicUser, ProtectSystem, etc.) is worth
  # more than the symmetry of an explicit bind address.

  ###### devpi — PyPI proxy
  services.devpi-server = {
    enable = true;
    host = bridgeIp;
    port = 3141;
  };

  ###### registry:2 — Docker/OCI pull-through cache
  # In proxy mode the registry is read-only by design, so enableDelete and
  # enableGarbageCollect are mutually exclusive with the proxy block (the
  # daemon exits with status=2/INVALIDARGUMENT if both are set). Cache
  # turnover happens via storage time-based eviction set in extraConfig.
  services.dockerRegistry = {
    enable = true;
    listenAddress = bridgeIp;
    port = 5000;
    storagePath = "/var/lib/cache/registry";
    extraConfig = {
      proxy = {
        remoteurl = "https://registry-1.docker.io";
      };
    };
  };

  ###### Verdaccio — npm registry proxy
  # No NixOS module in nixpkgs and no top-level pkgs.verdaccio derivation, so
  # we run the upstream OCI image via virtualisation.oci-containers. Storage
  # and config are bind-mounted from the host so we still have a single
  # source of truth for state.
  virtualisation.oci-containers.backend = lib.mkDefault "podman";
  virtualisation.oci-containers.containers.verdaccio = {
    image = "verdaccio/verdaccio:5";
    autoStart = true;
    ports = [ "${bridgeIp}:4873:4873" ];
    volumes = [
      "/var/lib/cache/verdaccio:/verdaccio/storage"
      "/etc/verdaccio/config.yaml:/verdaccio/conf/config.yaml:ro"
    ];
  };
  virtualisation.podman.enable = lib.mkDefault true;

  environment.etc."verdaccio/config.yaml".text = ''
    storage: /verdaccio/storage
    plugins: /verdaccio/plugins

    web:
      enable: true
      title: lagrange-verdaccio

    auth:
      htpasswd:
        file: /verdaccio/storage/htpasswd
        max_users: -1

    uplinks:
      npmjs:
        url: https://registry.npmjs.org/
        max_fails: 4
        fail_timeout: 5m
        cache: true

    packages:
      '@*/*':
        access: $all
        publish: $authenticated
        proxy: npmjs
      '**':
        access: $all
        publish: $authenticated
        proxy: npmjs

    server:
      keepAliveTimeout: 60

    middlewares:
      audit:
        enabled: true

    log:
      type: stdout
      format: pretty
      level: warn

    listen: 0.0.0.0:4873

    max_body_size: 100mb
  '';

  ###### nginx in front of crates.io — sparse-index aware HTTP cache
  # Cargo doesn't need a smart proxy; just a vanilla HTTP cache wins ~all of
  # the benefit because the sparse index is plain JSON GETs.
  services.nginx = {
    enable = true;
    recommendedProxySettings = true;
    recommendedTlsSettings = false;
    recommendedGzipSettings = true;

    appendHttpConfig = ''
      proxy_cache_path /var/lib/cache/cargo levels=1:2 keys_zone=cargo:32m
        max_size=30g inactive=30d use_temp_path=off;

      # Public DNS, not 127.0.0.53. systemd-resolved is NOT enabled on
      # this host, so pointing nginx at its stub listener produced a
      # 30s hang followed by 502 on every cargo index fetch (the
      # resolver never answered, the upstream was never reached). Use
      # the same upstream servers dnsmasq forwards to so we don't add
      # a new external dependency. Combined with proxy_pass via a
      # variable, this still gets lazy DNS — config-test doesn't try
      # to resolve upstreams (which is what makes the test VM with no
      # internet at config-test time work), and a transient outage at
      # one resolver doesn't kill nginx.
      resolver 1.1.1.1 8.8.8.8 valid=300s ipv6=off;
    '';

    virtualHosts."cargo-cache" = {
      listen = [{ addr = bridgeIp; port = cargoProxyPort; }];

      # Sparse index lookups: /index/<crate-path>.json
      locations."/index/" = {
        extraConfig = ''
          set $cargo_index "index.crates.io";
          proxy_pass https://$cargo_index/;
          proxy_cache cargo;
          proxy_cache_valid 200 5m;
          proxy_cache_valid 404 1m;
          proxy_set_header Host $cargo_index;
        '';
      };

      # Crate tarballs: /api/v1/crates/<name>/<version>/download
      locations."/api/" = {
        extraConfig = ''
          set $cargo_api "crates.io";
          proxy_pass https://$cargo_api/api/;
          proxy_cache cargo;
          proxy_cache_valid 200 30d;
          proxy_cache_valid 404 1m;
          proxy_set_header Host $cargo_api;
        '';
      };
    };
  };

  # Storage directories.
  # - atticd, athens use DynamicUser=true; their upstream modules set
  #   StateDirectory and create the path with the correct ownership. Do NOT
  #   add tmpfiles entries referencing a literal `athens`/`atticd` user —
  #   those users don't exist persistently and activation will fail.
  # - docker-registry, nginx are real users (or pkgs.nginx default group),
  #   so explicit rules are fine.
  # - verdaccio runs inside a podman container; the host-side dir just needs
  #   to be writable by the container's UID. podman maps root-in-container
  #   to root-on-host by default for rootful, which works.
  systemd.tmpfiles.rules = [
    "d /var/lib/cache/registry              0750 docker-registry docker-registry -"
    # Verdaccio's container image runs as the `verdaccio` user (UID 10001),
    # not root. With rootful podman, that UID is unmapped — the container
    # writes as host UID 10001. Pre-chown the bind mount so the in-container
    # process can create .sinopia-db.json on first start.
    "d /var/lib/cache/verdaccio             0750 10001      10001      -"
    "d /var/lib/cache/cargo                 0750 nginx      nginx      -"
    # atticd storage is BindPath'd by the hardened systemd unit; the
    # directory must exist before the namespace is constructed.
    "d /var/lib/cache/atticd                0750 atticd     atticd     -"
    "d /var/lib/cache/atticd/storage        0750 atticd     atticd     -"
  ];
  };
}
