{ self, nixpkgs, pkgs }:

# Cheap evaluation test: assert that lib.mkRepoVm with sample args produces a
# realizable NixOS config. Catches breakage in the per-VM builder before
# anything tries to actually boot it.

let
  sample = self.lib.mkRepoVm {
    name = "evaltest";
    repoUrl = "git@github.com:christopherjmiller/sample.git";
    branch = "main";
    vmIp = "10.42.0.10";
    vmMac = "02:00:00:00:00:0a";
  };
in
pkgs.runCommand "lagrange-repo-vm-eval"
  {
    drv = sample.config.system.build.toplevel.drvPath;
  }
  ''
    # The mere fact that we got here means evaluation succeeded. Record the
    # derivation path so the test output is non-empty.
    printf '%s\n' "$drv" > $out
  ''
