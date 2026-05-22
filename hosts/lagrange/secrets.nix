{ config, lib, pkgs, ... }:

{
  sops = {
    defaultSopsFile = ../../secrets/satellite.yaml;
    age = {
      keyFile = "/var/lib/sops-nix/key.txt";
      generateKey = false;
    };

    secrets = {
      comin-token = {
        mode = "0400";
        owner = "root";
        group = "root";
        restartUnits = [ "comin.service" ];
      };

      wg-private-key = {
        mode = "0400";
        owner = "root";
        group = "root";
        restartUnits = [ "wireguard-wg0.service" ];
      };

      admin-service-token = {
        mode = "0400";
        owner = "lagrange-admin";
        group = "lagrange-admin";
        restartUnits = [ "lagrange-admin.service" ];
      };

      atticd-env = {
        mode = "0400";
        owner = "atticd";
        group = "atticd";
        restartUnits = [ "atticd.service" ];
      };

      deploy-keys-tar = {
        mode = "0400";
        owner = "lagrange-admin";
        group = "lagrange-admin";
      };
    };
  };
}
