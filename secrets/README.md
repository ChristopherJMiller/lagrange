# `secrets/`

sops-nix-encrypted secrets. Plaintext never lands in git.

## Recipients (already set up)

Both age public keys are baked into `/.sops.yaml`:

| Role | Public key | Private key path |
|---|---|---|
| Operator | `age1pqld85e4ge2uy99g68g2ez27uy64y9kzw830dtgzfgw68gupapvq4avz6f` | `~/.config/sops/age/keys.txt` (shared with other projects) |
| Lagrange host | `age1468lcrw76fq5kecl52a6rk6s0v0yvryhpg475ym62w06f4excdxs7tpwwr` | `~/.config/sops/age/lagrange-host.txt` |

The host key was generated locally so the encrypted secrets can be prepared
*before* the box exists. It gets copied to the box at install time (below).

## Producing `secrets/satellite.yaml` (the encrypted file)

1. Generate fresh secret values on the workstation:

   ```sh
   # Bearer token (use same value in cluster Secret + lagrange secrets file)
   openssl rand -hex 32

   # Atticd RS256 signing secret
   openssl genrsa -traditional 4096 | base64 -w0
   # → put as ATTIC_SERVER_TOKEN_RS256_SECRET=<value> in atticd-env

   # WireGuard private key (will be regenerated on the box at first boot
   # instead — see "WG bootstrap" below; leave wg-private-key blank for now)

   # comin token — your GitHub PAT with `repo` scope for this repo
   ```

2. Fill in `secrets/satellite.yaml.example` with the values, write the result
   to `secrets/satellite.yaml`:

   ```sh
   cp secrets/satellite.yaml.example secrets/satellite.yaml
   # edit secrets/satellite.yaml with real values
   ```

3. Encrypt in place:

   ```sh
   sops --encrypt --in-place secrets/satellite.yaml
   ```

4. Commit. The encrypted form is safe to push to a public repo.

## On-box install: making the lagrange host key available

The installer ISO ships a `lagrange-install` wrapper that runs disko +
nixos-install and then **pauses** so you can drop the host age key onto the
freshly-installed rootfs before reboot. The flow:

```sh
# On the booted installer ISO, as root:
sudo lagrange-install                       # interactive disk pick
# …or scripted:
sudo lagrange-install --disk /dev/nvme0n1

# When it pauses with the "drop your key" banner, from your workstation:
scp ~/.config/sops/age/lagrange-host.txt root@<installer-ip>:/tmp/key.txt

# Back on the installer console, drop it in place and press Enter:
install -D -m 600 /tmp/key.txt /mnt/var/lib/sops-nix/key.txt
```

`lagrange-install` validates that `/mnt/var/lib/sops-nix/key.txt` is non-empty
before rebooting, so a missed paste won't silently brick first-boot
activation. On first boot, sops-nix uses that key to decrypt
`secrets/satellite.yaml`.

## WG bootstrap

The WG private key is best generated on the box at first boot and the public
key pasted back into the cluster's `wg-gateway.yaml`. Workflow:

```sh
# On Lagrange, after first boot, as root:
wg genkey | tee /tmp/wg-private | wg pubkey > /tmp/wg-public
cat /tmp/wg-public                 # paste into cluster/wg-gateway.yaml peer

# Then sops-encrypt the private key into satellite.yaml:
# On the operator workstation:
sops set secrets/satellite.yaml '["wg-private-key"]' "$(cat <transferred-key>)"
git commit -am "secrets: install wg key"
git push
# comin reconciles within 60s; wireguard-wg0.service restarts with the new key.
```

## Rotating a single secret

```sh
sops set secrets/satellite.yaml '["wg-private-key"]' '"NEW_VALUE"'
git commit -am "secrets: rotate wg key"
git push
```

comin redeploys and restarts the dependent service within 60s.
