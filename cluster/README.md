# Cluster-side manifests

These manifests run in the operator's Kubernetes cluster. They are *not*
managed by comin (comin only owns the Lagrange host config).

## Apply order

```sh
# 1. WireGuard gateway pod (so 10.99.0.2 becomes routable in-cluster).
kubectl apply -f wg-gateway.yaml

# 2. Service, Endpoints, and authentik-fronted Ingress for the admin API.
kubectl apply -f lagrange-admin.yaml
```

## Replacements before first apply

In `wg-gateway.yaml`:
- `REPLACE_WITH_CLUSTER_WG_PRIVATE_KEY` — `wg genkey` on a cluster node.
- `REPLACE_WITH_LAGRANGE_WG_PUBLIC_KEY` — `wg pubkey < /var/lib/wg/privatekey`
  on Lagrange after first boot.

In `lagrange-admin.yaml`:
- `REPLACE_WITH_BEARER_TOKEN_MATCHING_LAGRANGE` — same value as
  `admin-service-token` in `secrets/satellite.yaml`. Both must be identical.
- `authentik.example.com` and `lagrange.internal.example` — your real hosts.

## Smoke test

```sh
kubectl run -n ops curl --rm -it --image=curlimages/curl -- \
  curl -H "Authorization: Bearer $TOKEN" \
       https://lagrange-admin.ops.svc:8443/v1/health
```

Expected: `{"status":"ok","vms_running":0,"vms_total":0}`.

End-to-end through ingress:

```sh
curl https://lagrange.internal.example/v1/health
# → redirected to authentik for SSO; after login, returns the JSON above.
```
