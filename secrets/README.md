# `secrets/`

This directory holds **sops-nix-encrypted** secrets. Plaintext never lands
in git.

## First-time setup

1. Generate an age key on the operator workstation:

   ```sh
   age-keygen -o ~/.config/sops/age/keys.txt
   ```

   Note the public key it prints.

2. Generate an age key on the Lagrange host (after the first boot):

   ```sh
   sudo mkdir -p /var/lib/sops-nix
   sudo age-keygen -o /var/lib/sops-nix/key.txt
   sudo chmod 600 /var/lib/sops-nix/key.txt
   age-keygen -y /var/lib/sops-nix/key.txt   # prints public key
   ```

3. Edit `/.sops.yaml` and replace the two `age1__PLACEHOLDER_*__` lines with
   the public keys from steps 1 and 2.

4. Create the encrypted secrets file:

   ```sh
   cp secrets/satellite.yaml.example secrets/satellite.yaml
   # edit secrets/satellite.yaml to fill in real values
   sops --encrypt --in-place secrets/satellite.yaml
   ```

5. Commit `secrets/satellite.yaml` (encrypted) and `.sops.yaml`.
   Do **not** commit `satellite.yaml.example` with real secrets — it's a
   template only.

## Rotating a single secret

```sh
sops set secrets/satellite.yaml '["wg-private-key"]' '"NEW_VALUE"'
git commit -am "secrets: rotate wg key"
git push
# comin will redeploy + restart the dependent service within 60s.
```
