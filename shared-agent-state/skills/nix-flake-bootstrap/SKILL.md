---
name: nix-flake-bootstrap
description: Add a flake.nix to a repo that doesn't have one, with a devShell that captures the toolchain the repo actually uses.
---

# Bootstrap a flake.nix for this repo

Use this when the repo has no `flake.nix` and you need a reproducible
dev shell. The goal is the smallest possible change: just enough to
make `nix develop` give you the toolchain the repo actually uses.

## Procedure

1. **Survey the repo.** What languages and runtimes does it actually
   need? Look at:
   - `package.json` → node + npm/pnpm/yarn
   - `Cargo.toml` → rustc + cargo (note `rust-toolchain.toml` if present)
   - `pyproject.toml` / `requirements.txt` → python + uv/pip
   - `go.mod` → go
   - `Dockerfile` → may name the canonical runtime versions
   - CI configuration files for the canonical build commands

2. **Write a minimal `flake.nix`:**

   ```nix
   {
     description = "<repo description>";

     inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
     inputs.flake-utils.url = "github:numtide/flake-utils";

     outputs = { self, nixpkgs, flake-utils }:
       flake-utils.lib.eachDefaultSystem (system:
         let pkgs = nixpkgs.legacyPackages.${system}; in
         {
           devShells.default = pkgs.mkShell {
             packages = with pkgs; [
               # ...the toolchain you actually identified above
             ];
           };
         });
   }
   ```

3. **Verify it builds:** `nix develop --command true`.

4. **Commit + push.** This is a "discrete unit of work" by the standards
   in CLAUDE.md.

## When NOT to do this

- The repo already has a flake; just edit the existing one.
- The repo deliberately uses a non-Nix toolchain manager (mise, asdf,
  rtx). In that case, you're better off making the manager work in this
  environment than adding a flake — usually means a `nix-shell -p` to
  get the manager itself.
