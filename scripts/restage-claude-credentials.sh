#!/usr/bin/env bash
# One-shot re-stage of the operator's Claude Code login session into
# the lagrange admin service. Hides the bearer-token fetch + admin-API
# routing so a stale-token unstick is a single command.
#
# Reads:
#   ~/.claude/.credentials.json   (operator's local Claude oauth bundle)
#   ~/.claude.json                (operator's local Claude install file)
#
# Sends them to /v1/auth/claude-credentials on the admin service.
# Admin service is bound to the wireguard IP (10.99.0.2:8443) and not
# reachable from the laptop directly, so we tunnel via SSH to the
# satellite host and curl from there with the sops-managed bearer
# token. The credentials never touch the shell command line — they're
# piped through stdin.
#
# Usage:
#   scripts/restage-claude-credentials.sh                # uses default host
#   scripts/restage-claude-credentials.sh user@otherhost
#
# Environment overrides:
#   LAGRANGE_SATELLITE    SSH target for the host (default: 192.168.0.244)
#   LAGRANGE_ADMIN_URL    admin base URL on the satellite
#                         (default: http://10.99.0.2:8443)
#   LAGRANGE_TOKEN_PATH   sops-mounted bearer token path on the satellite
#                         (default: /run/secrets/admin-service-token)
#
# Why this matters: claude's oauth access tokens expire (~24h). Claude
# refreshes on use, but the bind-mount of .credentials.json into the
# guest is read-only-ish (atomic-rename fails on bind-mounted single
# files), so the refresh can't persist. Until the bind-mount is
# replaced with a write-through scheme, the cure is to re-stage from
# the laptop where you've just run `claude auth login`.

set -euo pipefail

SATELLITE="${1:-${LAGRANGE_SATELLITE:-192.168.0.244}}"
ADMIN_URL="${LAGRANGE_ADMIN_URL:-http://10.99.0.2:8443}"
TOKEN_PATH="${LAGRANGE_TOKEN_PATH:-/run/secrets/admin-service-token}"

CRED_PATH="${HOME}/.claude/.credentials.json"
INSTALL_PATH="${HOME}/.claude.json"

for need in jq ssh; do
  command -v "$need" >/dev/null 2>&1 || { echo "missing dependency: $need" >&2; exit 1; }
done

for p in "$CRED_PATH" "$INSTALL_PATH"; do
  if [[ ! -f "$p" ]]; then
    echo "missing $p — run 'claude auth login' on this machine first" >&2
    exit 1
  fi
done

# Build the JSON body locally so credentials aren't visible in the
# remote shell's argv/ps. `--rawfile` reads the file as a string;
# jq -n constructs the object.
BODY="$(jq -n \
  --rawfile c "$CRED_PATH" \
  --rawfile i "$INSTALL_PATH" \
  '{credentials_json: $c, claude_json: $i}')"

# Pre-flight: report what we're staging so the operator sees the
# refresh actually pick up the new expiry.
EXPIRES="$(jq -r '.claudeAiOauth.expiresAt // "unknown"' "$CRED_PATH")"
if [[ "$EXPIRES" != "unknown" ]]; then
  # ms epoch → human, allow stat-on-stat for cross-platform date
  EXPIRES_HUMAN="$(date -u -d "@$(( EXPIRES / 1000 ))" '+%Y-%m-%d %H:%M:%S UTC' 2>/dev/null \
    || date -u -r "$(( EXPIRES / 1000 ))" '+%Y-%m-%d %H:%M:%S UTC' 2>/dev/null \
    || echo "$EXPIRES")"
  echo "Staging credentials expiring at: $EXPIRES_HUMAN"
fi

echo "→ ${SATELLITE} → ${ADMIN_URL}"

# Pipe BODY to the satellite. The remote shell reads the bearer token
# locally (via sudo, since the file is 0400 root:root from sops) and
# curls the admin. -w prints the HTTP status so we can confirm 204.
RESP=$(ssh -o BatchMode=no "$SATELLITE" \
  'set -e
   TOKEN="$(sudo cat '"$TOKEN_PATH"')"
   curl -sS -X POST \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     --data-binary @- \
     -w "\n__HTTP_STATUS__:%{http_code}" \
     "'"$ADMIN_URL"'/v1/auth/claude-credentials"' \
  <<< "$BODY")

STATUS="${RESP##*__HTTP_STATUS__:}"
BODY_RESP="${RESP%__HTTP_STATUS__:*}"

if [[ "$STATUS" != "204" ]]; then
  echo "POST failed: HTTP $STATUS" >&2
  echo "$BODY_RESP" >&2
  exit 1
fi

echo "✓ staged (HTTP 204). restart any running vessel for the agent to pick up the refresh:"
echo "    curl from orbit's UI, or:"
echo "    ssh ${SATELLITE} 'sudo systemctl restart microvm@<name>'"
