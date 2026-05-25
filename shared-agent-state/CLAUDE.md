# Lagrange compute satellite environment

You are running inside a NixOS microVM dedicated to a single git repository,
hosted on a machine called "Lagrange". This environment has specific
conventions. Read this carefully before acting.

## What this environment is

- NixOS microVM, one repo per VM, isolated from other repos.
- Sessions here may run for days or weeks. Optimize for sustained, autonomous
  work, not quick fixes.
- The host machine is shared with other VMs working on other repos. Do not
  attempt to escape the VM or address other VMs directly.

## Filesystem layout

- `/home/agent/work/` — the repository. This is your working directory.
- `~/.claude/projects/, todos/, statsig/` — your persistent memory. Survives
  VM teardown.
- `~/.claude/skills/, commands/, CLAUDE.md` — read-only, host-managed.
- `/tmp/` — ephemeral, cleared on VM reboot.
- Anything outside `/home/agent/` and `/tmp/` should be treated as system
  state.

## Toolchains

This is NixOS. To make a tool available, prefer in order:

1. Add it to the repo's `flake.nix` devShell, then `nix develop`.
2. If no flake exists, propose creating one in your next response before
   reaching for ad-hoc installation. There is a skill named
   `nix-flake-bootstrap` for exactly this.
3. `nix shell nixpkgs#<pkg>` for one-shot interactive use.

**Do not** use `apt`, `dnf`, or `pip install --user`. These will appear to
work but produce a non-reproducible environment.

`cargo`, `npm`, `pip`, `uv`, `go` all work normally — they are configured to
use host-side caching proxies (see below).

## Package caches

Most package managers are pre-configured to route through `cache.internal`
(host-side pull-through proxies). They are fast and warm — prefer them
over hitting public registries directly.

| Tool             | Endpoint                                              |
|------------------|-------------------------------------------------------|
| npm registry     | `http://cache.internal:4873/`                         |
| Go proxy         | `http://cache.internal:3000` (then `direct`)          |
| PyPI index       | `http://cache.internal:3141/root/pypi/+simple/`       |
| Cargo            | sparse `http://cache.internal:7878/index/` (via cargo config) |
| Docker registry  | `http://cache.internal:5000` (pull-through)           |

Nix is the exception: the substituter is plain `cache.nixos.org` (the
default), not the local attic cache. The lagrange attic instance
requires a bearer token we haven't plumbed into guests yet, so leaving
it unconfigured avoids silent 401s. See the `## Nix` section below.

If a proxy endpoint fails, fall back to the public registry — that is
allowed, not blocked. The proxies are a performance optimization, not a
security boundary. (Host nftables, not the guest, decides egress.)

## Network

- You can reach the public internet (GitHub, the Anthropic API, npmjs,
  pypi, cache.nixos.org, etc.). Egress is open at the host level.
- You **cannot** reach other VMs on the cache bridge, and you cannot
  reach host services other than the cache layer listed above.
- `git push` works. Use it.

## Session hygiene

- This session may run for a long time. Compaction will happen even with the
  1M context. To preserve strategic context across compactions, keep a
  project-level `CLAUDE.md` in the repo itself updated with the north star
  for the current work.
- When you complete a discrete unit of work, **commit and push immediately**.
  Do not let uncommitted state accumulate across multiple work items. If the
  VM is destroyed, uncommitted state is lost. See the `commit-discipline`
  skill for the standard.
- Check `~/.claude/skills/` at the start of any new task. There may already
  be a skill for what you're about to do.

## Sudo

You have passwordless sudo inside this VM. The blast radius is contained to
this VM — `rm -rf /` here cannot harm the host or other VMs. Use it freely
for legitimate system-level operations (reading logs, restarting services
you own, etc.), but do not modify `/nix/store` or `/persistent/` outside
your home directory.

The setuid wrapper is at `/run/wrappers/bin/sudo` and `/run/wrappers/bin` is
first in `$PATH`. `which sudo` will show the wrapper. The non-setuid copy at
`/run/current-system/sw/bin/sudo` is just a symlink to the unwrapped store
binary — if you invoke it directly, sudo will fail with confusing errors
about setuid that look like a system bug. Use the unqualified `sudo` and
let PATH resolve.

If `sudo <something>` returns an error, read what failed: sudo itself
almost certainly succeeded and the underlying command is the one
complaining. "Sudo isn't working" is rarely the right diagnosis.

## Nix

`nix-daemon` is enabled and the flakes / nix-command experimental
features are on, so all of `nix develop`, `nix build`, `nix shell
nixpkgs#<pkg>`, `nix-shell -p <pkg>`, and `nix flake show` work
normally for the `agent` user.

`/nix/store` is a union mount:

- **Lower (read-only):** the host's `/nix/store`, virtiofs-shared at
  `/nix/.ro-store`. Everything the host has already realized is
  instantly available — no copy, no download.
- **Upper (writable):** a per-VM sparse ext4 image (~32 GiB max) at
  `/nix/.rw-store`. New derivations the VM builds, and anything pulled
  from substituters, land here.

The image is sparse (only consumes actual usage on the host disk) and
**persists across VM stop/start/restart**. It's wiped only when the
VM is destroyed (`microvm -d` / orbit "Destroy"). So you can build
once and have it cached for the next session.

Substituters: only `cache.nixos.org` (the nixpkgs default) is
configured. The host's local attic cache is not currently wired in.
For anything in nixpkgs, substitution works fine. For your own repo's
flake outputs, the first build inside the VM has to compile from
source — which is real work but won't fail.

Things to keep in mind:

- The overlay is 32 GiB max. If you fill it (`nix build` of a giant
  closure, repeated `nix develop` rebuilds), recover with
  `sudo nix-collect-garbage -d`. ext4 may not return the freed blocks
  to sparse on the host immediately; `sudo fstrim /nix/.rw-store`
  forces that if disk usage on the host matters.
- A `nix shell nixpkgs#foo` that pulls a huge closure still works,
  but if you find yourself doing it repeatedly, add the package to
  the repo's `flake.nix` devShell so the host realises it once and
  every VM sees it via the read-only lower layer (no per-VM copy).

## When something feels wrong

If a tool that should exist doesn't, or a cache that should be reachable
isn't, the right response is to investigate — not to find a workaround. The
satellite is intentionally tightly bounded; an "unexpected" failure is much
more often a real configuration issue than a problem with your task. Use
`vm-self-introspection` to check the basics before going deeper.
