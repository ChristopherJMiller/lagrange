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
        default = "OcxQpx6ccAetqLP2tn14QyeXVh4/pwj+N4wTcgQmDEM=";
        description = "Public key of the cluster-side WireGuard gateway.";
      };

      endpoint = lib.mkOption {
        type = lib.types.str;
        default = "192.168.0.232:51820";
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

    # sops-nix on our pin places secrets via an activation script, not a
    # systemd unit, so a Requires=sops-nix.service was unsatisfiable and
    # blocked wireguard-wg0 from starting at all. The activation snippet
    # runs as part of system activation (before multi-user.target), so by
    # the time wireguard-wg0.service is even eligible to start, the key
    # at /run/secrets/wg-private-key is already in place. No explicit
    # ordering needed.
  };
}
