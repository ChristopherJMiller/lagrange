---
name: vm-self-introspection
description: Check what resources are available before kicking off expensive work. Frees you to make informed choices about parallelism, batch size, and timeouts.
---

# Look around before going big

Before launching anything that takes more than a few minutes
(test suite, large build, batch job, anything spawning subagents),
spend 30 seconds confirming the VM has the headroom.

## Resources

```sh
# Memory: total/used/available, swap usage
free -h

# Disk: focus on /home/agent and /nix
df -h /home/agent /nix /tmp

# CPU: load over the last 1m/5m/15m vs nproc
uptime
nproc

# Per-process pressure right now
systemd-cgtop -n 1 --depth 2
```

If any of these are tight, prefer a smaller batch / shorter timeout / fewer
parallel jobs over kicking off and hoping. OOM-kill during a long-running
test produces a worse signal than an explicit smaller test.

## Toolchain state

```sh
# What's actually on PATH and what version
which node npm cargo rustc python3 go git
node --version || true
cargo --version || true
go version || true
python3 --version || true
```

If a tool you need is missing, the right move is usually `nix shell nixpkgs#<pkg>`
or (better) add it to the repo's `flake.nix` devShell — see the
`nix-flake-bootstrap` skill.

## Caches

```sh
# Quick health probe of the cache layer (none of these should fail)
for port in 8080 4873 3000 3141 7878 5000; do
  nc -zv cache.internal $port 2>&1 | head -n 1
done
```

If a cache is unreachable, use the `cache-debug` skill before doing
anything else — work will be slow and possibly fail.

## The point

This isn't ceremony. It's the difference between "the build failed" and
"the build failed because I was out of memory and could have used a smaller
parallelism." Once you've done it twice in a session, the picture is
loaded; you don't need to re-check unless something feels off.
