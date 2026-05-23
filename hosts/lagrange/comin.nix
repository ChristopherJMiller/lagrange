{ config, lib, pkgs, ... }:

{
  services.comin = {
    enable = true;
    hostname = "lagrange";

    # 60s reconcile loop. Day-2 ops are `git push` to the config repo.
    # The repo is public, so we clone unauthenticated. If/when we ever flip
    # to a private mirror, add `auth.access_token_path = ...;` here, point
    # comin-token in satellite.yaml at a real PAT, and the rest works.
    remotes = [{
      name = "origin";
      url = "https://github.com/christopherjmiller/lagrange.git";
      branches.main = { name = "main"; };
      poller.period = 60;
    }];

    # Comin's metrics exporter is for local Prometheus scraping. It does not
    # need to be reachable across the WG tunnel — and binding to 10.99.0.2
    # before wg0 is up made comin crashloop. Bind locally instead.
    exporter = {
      listen_address = "127.0.0.1";
      port = 4243;
      openFirewall = false;
    };
  };
}
