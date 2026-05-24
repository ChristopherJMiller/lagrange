{
  description = "Lagrange — NixOS compute satellite for Claude Code remote-control sessions";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-utils.url = "github:numtide/flake-utils";

    microvm.url = "github:microvm-nix/microvm.nix";
    microvm.inputs.nixpkgs.follows = "nixpkgs";

    comin.url = "github:nlewo/comin";
    comin.inputs.nixpkgs.follows = "nixpkgs";

    sops-nix.url = "github:Mic92/sops-nix";
    sops-nix.inputs.nixpkgs.follows = "nixpkgs";

    disko.url = "github:nix-community/disko";
    disko.inputs.nixpkgs.follows = "nixpkgs";

    attic.url = "github:zhaofengli/attic";
    attic.inputs.nixpkgs.follows = "nixpkgs";

    claude-code.url = "github:sadjow/claude-code-nix";
    claude-code.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    { self
    , nixpkgs
    , flake-utils
    , microvm
    , comin
    , sops-nix
    , disko
    , attic
    , claude-code
    , ...
    }@inputs:
    let
      system = "x86_64-linux";

      commonModules = [
        microvm.nixosModules.host
        comin.nixosModules.comin
        sops-nix.nixosModules.sops
        attic.nixosModules.atticd
        ./modules/cache-layer.nix
        ./modules/wireguard-tunnel.nix
        ./modules/shared-agent-state.nix
        ./modules/lagrange-admin.nix
        ./modules/repo-vm.nix
      ];

      specialArgs = {
        inherit inputs self;
        flakeInputs = inputs;
      };

      pkgsFor = nixpkgs.legacyPackages.${system};
    in
    {
      nixosConfigurations = {
        lagrange = nixpkgs.lib.nixosSystem {
          inherit system specialArgs;
          modules = commonModules ++ [
            disko.nixosModules.disko
            ./hosts/lagrange/disko.nix
            ./hosts/lagrange/default.nix
          ];
        };

        installer = nixpkgs.lib.nixosSystem {
          inherit system specialArgs;
          modules = [
            "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-minimal.nix"
            ./hosts/lagrange/installer.nix
          ];
        };
      };

      # Re-exported so tests and downstream flakes can pull individual modules
      # without pulling the whole host config.
      nixosModules = {
        lagrangeAdmin = { ... }: {
          imports = [ ./modules/lagrange-admin.nix ];
          _module.args = { inherit self; };
        };
        cacheLayer = ./modules/cache-layer.nix;
        wireguardTunnel = ./modules/wireguard-tunnel.nix;
        sharedAgentState = ./modules/shared-agent-state.nix;
        repoVm = ./modules/repo-vm.nix;
      };

      lib.mkRepoVm = import ./lib/repo-vm.nix {
        inherit nixpkgs microvm claude-code system;
      };

      packages.${system} = {
        lagrange-admin = pkgsFor.callPackage ./admin-service/package.nix {
          # self.rev is set when this flake input is a clean git commit;
          # self.dirtyRev gets a `-dirty` suffix when something is uncommitted;
          # both are absent when the source tree isn't a git repo at all
          # (`nix build` from a tarball). Walk down the cascade.
          rev = self.rev or self.dirtyRev or "unknown";
        };
        default = self.packages.${system}.lagrange-admin;
        installer-iso = self.nixosConfigurations.installer.config.system.build.isoImage;
      };

      checks.${system} = {
        # Cheap evaluation check; runs as part of `nix flake check`.
        repo-vm-eval = import ./tests/repo-vm-eval.nix {
          inherit self nixpkgs;
          pkgs = pkgsFor;
        };

        # Rust unit tests. Reuse the package build with doCheck = true; the
        # checkPhase from cargoCheckHook runs `cargo test` and fails the
        # derivation on any test failure.
        admin-service-tests = self.packages.${system}.lagrange-admin.overrideAttrs (old: {
          pname = "${old.pname}-tests";
          doCheck = true;
        });

        # Full module-to-binary smoke test in a NixOS VM.
        admin-service-smoke = import ./tests/admin-service-smoke.nix {
          inherit self nixpkgs;
          pkgs = pkgsFor;
        };

        # Cache layer integration test.
        cache-layer-up = import ./tests/cache-layer-up.nix {
          inherit self nixpkgs;
          pkgs = pkgsFor;
        };

        # Boots a NixOS VM with the same lib/repo-vm-guest.nix module
        # the real mkRepoVm composes, and asserts claude-remote stays
        # active + insteadOf is set correctly + publisher posts the
        # session URL. Catches the class of bugs that previously
        # required round-tripping to the satellite.
        repo-vm-boot = import ./tests/repo-vm-boot.nix {
          inherit self nixpkgs claude-code;
          pkgs = pkgsFor;
        };
      };

      devShells.${system}.default = pkgsFor.mkShell {
        name = "lagrange";

        packages = with pkgsFor; [
          # Nix tooling
          nixpkgs-fmt
          nil
          nix-tree
          # Secrets
          sops
          age
          # Rust toolchain (matches what package.nix uses)
          rustc
          cargo
          rustfmt
          clippy
          rust-analyzer
          # Native deps for cargo build
          gcc
          pkg-config
          openssl
          openssl.dev
          sqlite
          # Useful at the REPL
          jq
          ripgrep
          fd
          curl
          httpie
          # orbit/ frontend — vite dev server, vitest, tsc, etc.
          nodejs_22
          pnpm_9
          # Container build for orbit/ (used by k8s deploy)
          docker-client
          # Nix test running (qemu used by the test framework; pulling it in
          # makes test re-runs warm)
          qemu
        ];

        shellHook = ''
          echo "lagrange dev shell"
          echo
          echo "  cargo test                                          # Rust unit tests (fastest)"
          echo "  nix build .#checks.${system}.repo-vm-eval -L         # cheap eval check"
          echo "  nix build .#checks.${system}.admin-service-smoke -L  # VM smoke test"
          echo "  nix build .#checks.${system}.cache-layer-up -L       # VM cache test"
          echo "  nix flake check -L                                  # everything"
          echo
          echo "orbit/ frontend:"
          echo "  (cd orbit && pnpm install)                          # first time"
          echo "  (cd orbit && pnpm dev)                              # vite, proxies /v1 → \$VITE_API_TARGET"
          echo "  (cd orbit && pnpm build && pnpm preview)            # production bundle"
          echo
          echo "Tail a backgrounded build:   nix log <drv-path>"
          echo "Watch progress without tail: pass -L and DO NOT pipe through tail/grep."
        '';
      };

      formatter.${system} = pkgsFor.nixpkgs-fmt;
    };
}
