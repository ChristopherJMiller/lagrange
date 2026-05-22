# Lagrange

NixOS flake for a single-machine "compute satellite" hosting long-lived
Claude Code remote-control sessions, one repo per microVM.

- **Host:** NixOS, GitOps-managed by [comin](https://github.com/nlewo/comin).
- **VMs:** [microvm.nix](https://github.com/microvm-nix/microvm.nix) +
  cloud-hypervisor; one per repo, single agent session.
- **Caches:** attic, verdaccio, athens, devpi, cargo HTTP cache, registry:2 —
  all bound to the internal bridge, never the LAN.
- **Control plane:** [Rust admin service](admin-service/), bound to a
  WireGuard tunnel only. Fronted by the operator's existing
  [authentik](https://goauthentik.io/) ingress for human auth.

Jump to the [operator runbook](#operator-runbook) below.

## Tree

```
.
├── flake.nix                     # canonical entry point
├── hosts/lagrange/               # the box itself
│   ├── default.nix
│   ├── hardware.nix              # placeholder; replace from nixos-generate-config
│   ├── networking.nix            # cachebr0, NAT, nftables
│   ├── comin.nix                 # services.comin → this repo
│   ├── installer.nix             # bootstrap ISO config
│   └── secrets.nix               # sops-nix wiring
├── modules/                      # reusable host modules
│   ├── cache-layer.nix
│   ├── wireguard-tunnel.nix
│   ├── shared-agent-state.nix
│   ├── repo-vm.nix
│   └── lagrange-admin.nix
├── lib/repo-vm.nix               # mkRepoVm — builder used by admin service
├── admin-service/                # the Rust glue (Axum + sqlx)
├── shared-agent-state/           # CLAUDE.md + skills mounted into every VM
├── secrets/                      # sops-nix encrypted YAML
└── cluster/                      # K8s manifests (apply once on the cluster)
```

## Build & test

```sh
nix develop                                          # dev shell with everything
# or, if you use direnv: `direnv allow` (uses .envrc → use flake)

cargo test --bins                                     # Rust unit tests (seconds)
nix build .#checks.x86_64-linux.repo-vm-eval -L      # cheap config eval
nix build .#checks.x86_64-linux.admin-service-tests -L
nix build .#checks.x86_64-linux.admin-service-smoke -L  # VM test
nix build .#checks.x86_64-linux.cache-layer-up -L       # VM test
nix flake check -L                                    # everything

nix build .#installer-iso                             # bootstrap USB
nix build .#lagrange-admin                            # admin service binary
nixos-rebuild build --flake .#lagrange                # dry-run the host closure
```

### Watching progress

Use `nix build -L` (don't pipe through `tail` — it swallows the live stream).
For a backgrounded build, `nix log <drv-path>` reads the persistent log.

The first run of any VM test substitutes a lot from cache.nixos.org (NixOS
test infra, qemu, kernel). Subsequent runs are warm-cache and finish in
~30s for the cheap test, ~2–3 min for VM tests.

## Operator runbook

### Day 0 — bring up

1. **Make the secrets file.** See `secrets/README.md`. Encrypt with sops to
   `secrets/satellite.yaml`.
2. **Replace placeholders.** Search for `__PLACEHOLDER__` and
   `REPLACE_WITH_` across the tree. Every line that hits a real grep result
   needs a value before deploy.
3. **Build the installer ISO:** `nix build .#installer-iso` and write to
   USB.
4. **Boot Lagrange off the USB:** run `nixos-install --flake
   github:christopherjmiller/lagrange#lagrange`. Reboot.
5. **Place the host's sops age key** at `/var/lib/sops-nix/key.txt`. The
   key was generated in step 1 of `secrets/README.md`.
6. **Apply the cluster manifests:** `kubectl apply -f cluster/`.
7. **Verify the WG tunnel:** from a cluster node,
   `kubectl exec -n ops deploy/wg-gateway -- ping 10.99.0.2`.
8. **Smoke test the admin API:** `curl -H "Authorization: Bearer $TOKEN"
   http://lagrange-admin.ops.svc:8443/v1/health`.

### Day 1+ — daily ops

| Goal | How |
|------|-----|
| Spin up a repo-VM | `POST /v1/repos { name, repo_url, branch }` via the cluster ingress |
| Stop a repo-VM | `POST /v1/repos/{name}/stop` |
| Restart a repo-VM | `POST /v1/repos/{name}/restart` |
| Destroy a repo-VM (keep state) | `DELETE /v1/repos/{name}` |
| Destroy and wipe state | `DELETE /v1/repos/{name}?wipe_persistent=true` |
| Tail a repo-VM's claude-remote journal | `GET /v1/repos/{name}/logs?lines=200` |
| Update Claude Code globally | bump `claude-code` flake input, `git push`, comin applies in ≤60s |
| Add a new skill for all VMs | drop directory under `shared-agent-state/skills/`, `git push` |
| Update the user-level CLAUDE.md | edit `shared-agent-state/CLAUDE.md`, `git push` |

### Break-glass

The host itself accepts SSH from a single LAN IP defined in
`hosts/lagrange/networking.nix` (`iifname enp4s0 tcp dport 22 ip saddr ...`).
Use this for hardware-level recovery only — drift on the running config
gets clobbered by comin's next reconcile.

## Status

This is the v1 design. Several follow-ups are explicitly deferred:

- GPU passthrough (§11.4 of the design doc)
- Per-VM egress policies (§7.4)
- Public-internet exposure (we don't, by design)
- Multi-satellite federation (one box for now)
- `--headless` claude remote-control — currently wrapped in tmux as a
  workaround
