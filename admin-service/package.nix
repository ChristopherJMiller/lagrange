{ lib, rustPlatform, pkg-config, openssl, sqlite, rev ? "unknown" }:

rustPlatform.buildRustPackage {
  pname = "lagrange-admin";
  version = "0.1.0";

  src = ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
    # If sqlx/rustls/etc. eventually pull in unpublished deps, add their
    # outputHashes here. For now the lockfile-only path works.
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl sqlite ];

  # Surface the source rev to the binary via env → option_env! so
  # /v1/health can say which commit is actually running on the host.
  # flake.nix passes self.rev (clean) or self.dirtyRev (uncommitted);
  # falls through to "unknown" outside a flake context. NOT placed in
  # `version` above — changing the derivation name on every commit
  # would defeat caching for unrelated rebuilds.
  LAGRANGE_REV = rev;

  # sqlx::migrate!("./migrations") needs the migrations in the source tree
  # at build time; buildRustPackage already copies the whole src so no
  # special handling needed. sqlx::query doesn't use compile-time checking,
  # so we don't need SQLX_OFFLINE.

  # Tests run via the `admin-service-tests` check derivation in the flake
  # (which overrides this back to true). Don't run them on every system
  # rebuild — that would also block `nixos-install` on cargo test output.
  doCheck = false;

  meta = with lib; {
    description = "Lagrange — repo-VM lifecycle service for Claude Code sessions";
    mainProgram = "lagrange-admin";
    license = licenses.mit;
    platforms = platforms.linux;
  };
}
