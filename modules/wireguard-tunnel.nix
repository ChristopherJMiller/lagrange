{ config, lib, pkgs, ... }:

# Outbound-only WireGuard tunnel from Lagrange to the operator's K8s cluster.
#
# Direction matters: Lagrange initiates the connection so that:
#   - Home routers don't need a port forward.
#   - The cluster has a stable public endpoint; the home IP can change.
#   - PersistentKeepalive = 25 keeps the NAT mapping alive.
#
# The admin service (modules/lagrange-admin.nix) binds ONLY to 10.99.0.2 — even
# if nftables were misconfigured, the service wouldn't be reachable from the
# LAN because it isn't listening there.

let
  cfg = config.lagrange.wireguard;
in
{
  options.lagrange.wireguard = {
    interface = lib.mkOption {
      type = lib.types.str;
      default = "wg0";
      description = "WireGuard interface name.";
    };

    localAddress = lib.mkOption {
      type = lib.types.str;
      default = "10.99.0.2";
      description = "This host's address inside the WG tunnel.";
    };

    clusterPeer = {
      publicKey = lib.mkOption {
        type = lib.types.str;
        # Replace before deploying; this placeholder is intentionally invalid.
        default = "REPLACE_WITH_CLUSTER_GATEWAY_PUBKEY=";
        description = "Public key of the cluster-side WireGuard gateway.";
      };

      endpoint = lib.mkOption {
        type = lib.types.str;
        default = "wg.cluster.internal.example:51820";
        description = "Public endpoint of the cluster-side gateway.";
      };

      address = lib.mkOption {
        type = lib.types.str;
        default = "10.99.0.1";
        description = "Cluster gateway's address inside the tunnel.";
      };
    };

    persistentKeepalive = lib.mkOption {
      type = lib.types.int;
      default = 25;
      description = "Seconds between keepalive packets (NAT traversal).";
    };
  };

  config = {
    networking.wireguard.enable = true;

    networking.wireguard.interfaces.${cfg.interface} = {
      ips = [ "${cfg.localAddress}/32" ];
      listenPort = null; # Outbound-only; no inbound port.
      privateKeyFile = config.sops.secrets.wg-private-key.path;

      peers = [{
        publicKey = cfg.clusterPeer.publicKey;
        allowedIPs = [ "${cfg.clusterPeer.address}/32" ];
        endpoint = cfg.clusterPeer.endpoint;
        persistentKeepalive = cfg.persistentKeepalive;
      }];
    };

    # Make sure WG comes up after sops has placed the private key.
    systemd.services."wireguard-${cfg.interface}" = {
      after = [ "sops-nix.service" ];
      requires = [ "sops-nix.service" ];
    };
  };
}
