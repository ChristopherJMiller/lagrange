# Cluster-side manifests — moved

The cluster-side manifests (WireGuard gateway, lagrange-admin Service /
Ingress) used to live here. They now live in the operator's homeops repo
(`luma-homeops`) and are applied by Flux / kubectl from there.

This directory is kept only as a breadcrumb. Don't add yaml here.

## What the cluster owes Lagrange

The host config in this repo assumes the cluster provides:

1. **A WireGuard gateway** reachable from the LAN (or public internet) on
   UDP/51820. Its public key + endpoint are pinned in
   `modules/wireguard-tunnel.nix` as `clusterPeer.publicKey` /
   `clusterPeer.endpoint`. Lagrange dials out to it; the cluster never
   initiates inbound to Lagrange.

2. **Routing for `10.99.0.0/24`** inside the cluster's pod network, so
   that consumers can reach the Lagrange admin API at `10.99.0.2:8443`
   over the tunnel.

3. **A bearer token** matching `admin-service-token` in
   `secrets/satellite.yaml`. The cluster side holds the same value (as
   a Kubernetes Secret in luma-homeops) and passes it as
   `Authorization: Bearer …` on every API call.

## What Lagrange owes the cluster

1. **Its WireGuard public key**, derivable on the box with
   `sudo cat /run/secrets/wg-private-key | wg pubkey`.
   Provide this to the cluster operator when (re)keying — the cluster's
   wg-gateway needs it in its `[Peer]` block with
   `AllowedIPs = 10.99.0.2/32`.

2. **A reachable admin API on `10.99.0.2:8443`** once both sides of the
   tunnel are up. `GET /v1/health` returns `{"status":"ok",…}` when the
   bearer token matches.

## Smoke test

From a pod inside the cluster (run by luma-homeops):

```sh
kubectl run -n ops curl --rm -it --image=curlimages/curl -- \
  curl -H "Authorization: Bearer $TOKEN" \
       https://lagrange-admin.ops.svc:8443/v1/health
```

Expected: `{"status":"ok","vms_running":0,"vms_total":0}`.
