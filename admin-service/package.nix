{ lib, rustPlatform, pkg-config, openssl, sqlite }:

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
