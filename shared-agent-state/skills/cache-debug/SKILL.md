---
name: cache-debug
description: Diagnose why a package-manager fetch is failing inside the VM. Walks the cache.internal endpoints.
---

# Debug a failing package fetch

The VM's package managers route through `cache.internal` (10.42.0.1 on the
internal bridge). If a fetch fails, work the layers from the bottom up.

## Step 1 — DNS

```sh
getent hosts cache.internal
# Should print: 10.42.0.1  cache.internal
```

If this returns nothing, the bridge dnsmasq isn't reachable. Probably a
networking misconfiguration on the host. Open `/etc/resolv.conf` — the
nameserver should be `10.42.0.1`.

## Step 2 — TCP connectivity to the specific cache

| Tool | Port |
|------|------|
| attic (Nix substituter) | 8080 |
| verdaccio (npm) | 4873 |
| athens (Go) | 3000 |
| devpi (PyPI) | 3141 |
| cargo HTTP cache | 7878 |
| registry (Docker/OCI) | 5000 |

```sh
nc -zv cache.internal 4873   # adjust port per tool
```

## Step 3 — HTTP probe

```sh
curl -fsSL http://cache.internal:8080/  # attic root
curl -fsSL http://cache.internal:4873/  # verdaccio web UI
curl -fsSL http://cache.internal:3000/ping  # athens
```

If TCP succeeds but HTTP returns 5xx, the daemon is up but unhealthy.
Surface this to the operator; do not work around by hitting the public
registry directly.

## Step 4 — Tool-specific configuration sanity

| Tool | Where to look |
|------|---------------|
| Nix | `/etc/nix/nix.conf` substituters line |
| npm | `npm config get registry` |
| Go | `echo $GOPROXY` |
| pip / uv | `echo $PIP_INDEX_URL $UV_INDEX_URL` |
| Cargo | `~/.cargo/config.toml` `[source.lagrange]` block |
| Docker | `~/.docker/config.json` or pull via `cache.internal:5000/<image>` |

## Step 5 — Public-internet fallback (last resort)

If a single fetch absolutely must reach a public registry — for example
when the cache layer is genuinely down and the work cannot wait — set
the tool's variable explicitly *for that command only*, never persistently.

```sh
GOPROXY=direct go mod download   # one shot
npm install --registry=https://registry.npmjs.org/   # one shot
```

Never rewrite `/etc/nix/nix.conf` or shell rc files to bypass `cache.internal`
— that change will be silently lost on the next host comin sync, but in the
meantime you've burned hours of debugging time off the team's clock.
