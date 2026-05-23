# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Scope

This is the **host-side** repo for Lagrange — a single NixOS box that runs
many microVMs, one per repo, each hosting a long-lived Claude Code session.
You are almost certainly editing this from outside the satellite (your
workstation or another machine).

Not to be confused with `shared-agent-state/CLAUDE.md`, which is the
*guest-side* CLAUDE.md mounted read-only into every repo-VM. Editing that
file changes the agent prompt for every running session on the box; editing
this file only affects local tooling.

## Dev environment

Always use the flake devShell. `nix develop` (or `direnv allow` — `.envrc`
is `use flake`) is the supported entry point. Do not reach for ad-hoc
`nix-shell -p` — it bypasses the pinned Rust toolchain and native deps that
match `admin-service/package.nix`.

## Build & test

```sh
cargo test                                              # Rust unit tests (~seconds, run from admin-service/ or the workspace)
nix build .#checks.x86_64-linux.repo-vm-eval -L         # cheap eval check — does mkRepoVm still evaluate?
nix build .#checks.x86_64-linux.admin-service-tests -L  # Rust tests inside a Nix derivation
nix build .#checks.x86_64-linux.admin-service-smoke -L  # full NixOS VM test of the admin module
nix build .#checks.x86_64-linux.cache-layer-up -L       # NixOS VM test of the cache daemons
nix flake check -L                                      # everything

nix build .#installer-iso                               # bootstrap USB image
nix build .#lagrange-admin                              # admin service binary
nixos-rebuild build --flake .#lagrange                  # dry-run the host closure
```

Watching: pass `-L` and **don't** pipe through `tail`/`grep` — Nix
suppresses the live stream when stdout is not a TTY. For a backgrounded
build, `nix log <drv-path>` reads the persistent log.

The first VM test build pulls a lot from cache.nixos.org. Subsequent runs
are warm-cache and finish in ~30s (cheap) / ~2–3 min (VM).

## Architecture

Three layers, top-down:

**1. Host (NixOS, GitOps).** `flake.nix` exposes
`nixosConfigurations.lagrange`. Comin polls this repo every 60s
(`hosts/lagrange/comin.nix`) and applies `main`. The host runs no
public-facing services: the admin API is bound to the WireGuard endpoint
(`10.99.0.2`); the cache daemons are bound to the cache bridge
(`10.42.0.1`); only SSH from one LAN IP is exposed. Drift on the running
config gets clobbered by comin's next reconcile — do not rely on
hand-edits.

**2. Admin service (Rust, Axum + sqlx + SQLite).** `admin-service/` —
lifecycle for repo-VMs over a small HTTP API (`/v1/repos`, `/v1/health`,
`…/start|stop|restart|logs`, …). Bearer-token auth (token from sops via
`tokenFile`). Per-VM operations are serialized via an in-process lock
(`AppState::lock_vm`) so concurrent create/destroy can't race.

  On create, the service: (a) validates repo reachability with `git
  ls-remote`, (b) allocates an IP from `LAGRANGE_IP_POOL` and inserts a row,
  (c) **writes a tiny per-VM flake** to
  `/var/lib/lagrange-admin/vm-flakes/<name>/flake.nix` that imports this
  repo and calls `lib.mkRepoVm` with the per-VM args, (d) shells out to
  `microvm -c` and starts the systemd unit. Teardown reverses this. The
  service shells out to `systemctl`, `journalctl`, `microvm`, and
  `nixos-rebuild` via a sudo allowlist defined in `modules/lagrange-admin.nix`.

**3. Repo-VMs (microvm.nix / cloud-hypervisor).** Each VM is one
`mkRepoVm` evaluation (`lib/repo-vm.nix`). Cloud-hypervisor is chosen over
firecracker specifically because we need `virtiofs` to share `/nix/store`
read-only into the guest. Three virtiofs shares per VM: the host store,
`/var/lib/agent-shared` (= `shared-agent-state/` — CLAUDE.md/skills),
and the per-VM persistent volume `/var/lib/agent-state/<name>/`. The
agent's `~/.claude/{projects,todos,statsig}` and `~/.ssh` are bind-mounted
from `/persistent/...` so they survive `microvm -R`.

### Layout

```
flake.nix                  canonical entry — exposes nixosConfigurations, packages, checks, devShell
hosts/lagrange/            the satellite host config (boots NixOS, hosts microVMs)
modules/                   reusable host modules (cache-layer, wireguard, admin, repo-vm wiring)
lib/repo-vm.nix            mkRepoVm — guest config; called from per-VM flakes stamped by the admin service
admin-service/             Rust API: api.rs (HTTP), vm.rs (shellout), db.rs (sqlx), config.rs (env)
shared-agent-state/        mounted read-only into every VM; edits here ship to all sessions via comin
secrets/                   sops-encrypted YAML; plaintext never lands in git
tests/                     Nix VM tests + cheap eval check; tests/lib.nix trims closure for faster cold builds
cluster/                   README pointer — actual cluster-side manifests live in the operator's luma-homeops repo
```

## Cross-cutting invariants

- **PERSISTENT_SUBDIRS lockstep.** The list of persistent-volume
  subdirectories appears in both `lib/repo-vm.nix` (`systemd.tmpfiles` +
  `fileSystems`) and `admin-service/src/vm.rs::PERSISTENT_SUBDIRS`. When
  you add or rename a persistent path, change both.

- **Nix injection guard.** `admin-service/src/vm.rs::nix_str` escapes
  `${` so a repo URL or branch name containing `${...}` can't inject
  arbitrary Nix into the per-VM flake. Any new string interpolated into a
  generated flake must go through `nix_str`.

- **Module options must support the test override.** The admin module
  exposes `requireWireguard` / `requireSops` / `bindAddress` / `tokenFile`
  precisely so `tests/admin-service-smoke.nix` can run without a real
  tunnel or sops key. Adding a new hard dependency to the unit (e.g. a new
  `requires=`) needs a matching option, or the smoke test will deadlock.

- **Placeholders.** Before deploy, grep the tree for `__PLACEHOLDER__` and
  `REPLACE_WITH_`. Every hit needs a real value. CI doesn't catch these.

- **No public exposure, by design.** The admin API binds to the WireGuard
  IP, never `0.0.0.0`. Cache daemons bind to the bridge IP, never the LAN
  interface. Tests override these to `127.0.0.1` — do not copy that
  pattern into production module defaults.

## CI

`.github/workflows/ci.yml` runs `eval` (flake show, lagrange+installer
toplevel eval, admin build, cheap checks) and then `vm-tests` (the full VM
checks). Match locally with `nix flake check -L` before pushing.
