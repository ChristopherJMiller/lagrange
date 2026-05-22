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

All package managers are pre-configured to route through `cache.internal`:

| Tool             | Endpoint                                              |
|------------------|-------------------------------------------------------|
| Nix substituter  | `http://cache.internal:8080/lagrange`                 |
| npm registry     | `http://cache.internal:4873/`                         |
| Go proxy         | `http://cache.internal:3000` (then `direct`)          |
| PyPI index       | `http://cache.internal:3141/root/pypi/+simple/`       |
| Cargo            | sparse `http://cache.internal:7878/index/` (via cargo config) |
| Docker registry  | `http://cache.internal:5000` (pull-through)           |

If a fetch fails with a connection error to a public registry, that is
expected — the VM has restricted egress. Use the proxies; do not work around
them. See the `cache-debug` skill.

## Network

- You can reach the public internet for things the agent needs (GitHub, the
  Anthropic API, etc.).
- You **cannot** reach other VMs or the host's internal services beyond the
  cache layer.
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
for legitimate system-level operations (installing system packages via Nix,
reading logs, etc.), but do not modify `/nix/store` or `/persistent/` outside
your home directory.

## When something feels wrong

If a tool that should exist doesn't, or a cache that should be reachable
isn't, the right response is to investigate — not to find a workaround. The
satellite is intentionally tightly bounded; an "unexpected" failure is much
more often a real configuration issue than a problem with your task. Use
`vm-self-introspection` to check the basics before going deeper.
