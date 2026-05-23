{ config, lib, pkgs, ... }:

{
  services.comin = {
    enable = true;
    hostname = "lagrange";

    # 60s reconcile loop. Day-2 ops are `git push` to the config repo.
    remotes = [{
      name = "origin";
      url = "https://github.com/christopherjmiller/lagrange.git";
      auth.access_token_path = config.sops.secrets.comin-token.path;
      auth.username = "comin";
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
