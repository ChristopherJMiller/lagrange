#!/usr/bin/env bash
# Stage (or restage) a Sentry MCP OAuth bundle into lagrange under
# an operator-chosen alias. Mirrors restage-claude-credentials.sh:
# we hide the bearer-token fetch + admin-API routing behind a single
# command and never put the bundle on the shell command line.
#
# Prereq: on this laptop, you've run `claude` interactively and let it
# complete the OAuth handshake against mcp.sentry.dev (it pops a
# browser window the first time you ask claude to call a sentry tool).
# That handshake writes the bundle to mcpServers.sentry.oauth in
# ~/.claude.json. This script extracts it and POSTs it to admin.
#
# Usage:
#   scripts/restage-sentry-mcp.sh <alias>
#   scripts/restage-sentry-mcp.sh <alias> user@otherhost
#
# Example:
#   scripts/restage-sentry-mcp.sh backend
#
# Environment overrides (same as restage-claude-credentials.sh):
#   LAGRANGE_SATELLITE    SSH target (default: 192.168.0.244)
#   LAGRANGE_ADMIN_URL    admin base URL (default: http://10.99.0.2:8443)
#   LAGRANGE_TOKEN_PATH   sops-mounted bearer token path on the satellite
#                         (default: /run/secrets/admin-service-token)
#
# After this:
#   - VMs you create with --sentry-account=<alias> will have
#     mcpServers.sentry pre-staged.
#   - Existing VMs assigned to <alias> get their .claude.json restaged
#     automatically; restart them to pick up the new bundle.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <alias> [satellite-ssh-target]" >&2
  exit 2
fi
ALIAS="$1"
SATELLITE="${2:-${LAGRANGE_SATELLITE:-192.168.0.244}}"
ADMIN_URL="${LAGRANGE_ADMIN_URL:-http://10.99.0.2:8443}"
TOKEN_PATH="${LAGRANGE_TOKEN_PATH:-/run/secrets/admin-service-token}"

INSTALL_PATH="${HOME}/.claude.json"

for need in jq ssh; do
  command -v "$need" >/dev/null 2>&1 || { echo "missing dependency: $need" >&2; exit 1; }
done
if [[ ! -f "$INSTALL_PATH" ]]; then
  echo "missing $INSTALL_PATH — run claude on this machine and complete sentry OAuth first" >&2
  exit 1
fi

# Pull the oauth sub-object out. The whole mcpServers.sentry block has
# the URL/transport too but admin only wants the credential portion;
# stage_for_vm reconstructs the URL/transport at splice time.
OAUTH="$(jq -e '.mcpServers.sentry.oauth // empty' "$INSTALL_PATH" 2>/dev/null || true)"
if [[ -z "$OAUTH" ]]; then
  echo "no .mcpServers.sentry.oauth in $INSTALL_PATH — open claude on this" >&2
  echo "machine, ask it to use a sentry tool, complete the browser OAuth," >&2
  echo "then re-run this script." >&2
  exit 1
fi

# Sanity preview so the operator sees roughly what's being staged.
ACCESS_PREVIEW="$(echo "$OAUTH" | jq -r '.accessToken // "??"' | head -c 12)"
EXPIRES_AT="$(echo "$OAUTH" | jq -r '.expiresAt // empty')"
if [[ -n "$EXPIRES_AT" ]]; then
  # Sentry's expiresAt is ms epoch like Claude's.
  EXPIRES_HUMAN="$(date -u -d "@$(( EXPIRES_AT / 1000 ))" '+%Y-%m-%d %H:%M:%S UTC' 2>/dev/null \
    || date -u -r "$(( EXPIRES_AT / 1000 ))" '+%Y-%m-%d %H:%M:%S UTC' 2>/dev/null \
    || echo "$EXPIRES_AT")"
  echo "Staging sentry bundle '$ALIAS' (accessToken ${ACCESS_PREVIEW}…, expires $EXPIRES_HUMAN)"
else
  echo "Staging sentry bundle '$ALIAS' (accessToken ${ACCESS_PREVIEW}…, no expiresAt in bundle)"
fi

# Build the request body. The admin endpoint wants { bundle: <oauth-obj> }.
BODY="$(jq -n --argjson b "$OAUTH" '{bundle: $b}')"

echo "→ ${SATELLITE} → ${ADMIN_URL}/v1/auth/sentry-accounts/${ALIAS}"

RESP=$(ssh -o BatchMode=no "$SATELLITE" \
  'set -e
   TOKEN="$(sudo cat '"$TOKEN_PATH"')"
   curl -sS -X PUT \
     -H "Authorization: Bearer $TOKEN" \
     -H "Content-Type: application/json" \
     --data-binary @- \
     -w "\n__HTTP_STATUS__:%{http_code}" \
     "'"$ADMIN_URL"'/v1/auth/sentry-accounts/'"$ALIAS"'"' \
  <<< "$BODY")

STATUS="${RESP##*__HTTP_STATUS__:}"
BODY_RESP="${RESP%__HTTP_STATUS__:*}"

if [[ "$STATUS" != "204" ]]; then
  echo "PUT failed: HTTP $STATUS" >&2
  echo "$BODY_RESP" >&2
  exit 1
fi

echo "✓ staged (HTTP 204). VMs assigned to '$ALIAS' will pick this up on"
echo "  their next claude-remote restart. Restart one manually with:"
echo "    ssh ${SATELLITE} 'sudo systemctl restart microvm@<name>'"
