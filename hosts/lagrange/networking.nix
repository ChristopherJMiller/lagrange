{ config, lib, pkgs, ... }:

let
  # The cache bridge that all repo-VMs attach to. Host owns .1, VMs get .10+.
  cacheBridgeIfname = "cachebr0";
  cacheBridgeAddr = "10.42.0.1";
  cacheBridgeCidr = 24;

  # Adjust to whatever the box's primary NIC is on the home LAN.
  primaryWanIface = "enp4s0";
in
{
  networking = {
    # We manage the firewall ourselves via nftables (deny-by-default).
    firewall.enable = false;
    nftables.enable = true;

    useNetworkd = true;
    useDHCP = false;

    # Cache bridge: VM tap interfaces enslave to this; host runs the cache
    # daemons on the bridge IP.
    bridges.${cacheBridgeIfname} = {
      interfaces = [ ];
    };

    interfaces.${cacheBridgeIfname}.ipv4.addresses = [{
      address = cacheBridgeAddr;
      prefixLength = cacheBridgeCidr;
    }];

    interfaces.${primaryWanIface}.useDHCP = true;

    nat = {
      enable = true;
      internalInterfaces = [ cacheBridgeIfname ];
      externalInterface = primaryWanIface;
    };

    # cache.internal → 10.42.0.1 inside the VM bridge.
    extraHosts = ''
      ${cacheBridgeAddr} cache.internal
    '';
  };

  # dnsmasq resolves cache.internal for guests and offers DHCP on the bridge.
  # We deliberately leave the upstream resolver to the guest's systemd-resolved
  # so public DNS doesn't funnel through here.
  services.dnsmasq = {
    enable = true;
    settings = {
      interface = cacheBridgeIfname;
      bind-interfaces = true;
      no-resolv = true;
      port = 53;
      address = [ "/cache.internal/${cacheBridgeAddr}" ];
      dhcp-range = [ "${cacheBridgeIfname},10.42.0.200,10.42.0.250,1h" ];
      # Server-side static leases are added by the admin service via a
      # drop-in conf file at /etc/dnsmasq.d/leases.conf.
      conf-dir = "/etc/dnsmasq.d,*.conf";
    };
  };

  # Deny-by-default nftables ruleset. The single inbound exception is the WG
  # interface on the admin port. Defense-in-depth: the admin service also
  # binds only to the WG IP, never 0.0.0.0.
  networking.nftables.ruleset = ''
    table inet filter {
      chain input {
        type filter hook input priority 0; policy drop;

        iifname "lo" accept
        ct state established,related accept
        ct state invalid drop

        # ICMP for path-MTU and basic health.
        ip protocol icmp accept
        ip6 nexthdr icmpv6 accept

        # WireGuard tunnel — the cluster reaches us through wg0 on TCP/8443.
        iifname "wg0" tcp dport 8443 accept

        # Cache bridge: VMs talk to host-side caches.
        iifname "${cacheBridgeIfname}" tcp dport { 53, 80, 3000, 3141, 4873, 5000, 7878, 8080 } accept
        iifname "${cacheBridgeIfname}" udp dport 53 accept

        # SSH from a single break-glass IP on the LAN. Adjust as needed; the
        # default placeholder is intentionally narrow.
        iifname "${primaryWanIface}" tcp dport 22 ip saddr 192.168.1.100 accept

        log prefix "nft-input-drop: " level info limit rate 5/minute drop
      }

      chain forward {
        type filter hook forward priority 0; policy drop;

        ct state established,related accept
        # VM egress to the public internet (NATed).
        iifname "${cacheBridgeIfname}" oifname "${primaryWanIface}" accept
      }

      chain output {
        type filter hook output priority 0; policy accept;
      }
    }
  '';

  # network-online.target hangs forever otherwise on bridge-only setups.
  systemd.services.systemd-networkd-wait-online.enable = lib.mkForce false;
}
