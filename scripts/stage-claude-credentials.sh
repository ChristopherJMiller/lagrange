#!/usr/bin/env bash
# Stage the operator's Claude Code full-scope login session into the
# lagrange admin service. Each repo-VM picks the credentials up at
# create/restart time and uses them to register a Remote Control session
# visible at claude.ai/code.
#
# Run on the workstation where `claude auth login` was executed. Reads
# both files the Claude CLI persists, JSON-encodes them, and POSTs to
# /v1/auth/claude-credentials.
#
# Usage:
#   scripts/stage-claude-credentials.sh <base-url> <bearer-token-source>
#
# Example (in-cluster via kubectl):
#   scripts/stage-claude-credentials.sh \
#     http://lagrange-admin.ops.svc:8443 \
#     /tmp/lagrange-admin-token
#
# Example (over SSH to the lagrange host, bypassing the cluster):
#   scripts/stage-claude-credentials.sh \
#     http://10.99.0.2:8443 \
#     <(ssh chris@<host> 'sudo cat /run/secrets/admin-service-token')

set -euo pipefail

if [[ $# -ne 2 ]]; then
  cat >&2 <<EOF
Usage: $0 <base-url> <bearer-token-source>

  base-url             e.g. http://lagrange-admin.ops.svc:8443
                          or http://10.99.0.2:8443
  bearer-token-source  path to a file holding the admin-service bearer
                          token. Use process substitution to source from
                          a command, e.g. <(ssh ... 'sudo cat /run/...')
EOF
  exit 2
fi

BASE_URL="$1"
TOKEN_SRC="$2"

CRED_PATH="${HOME}/.claude/.credentials.json"
INSTALL_PATH="${HOME}/.claude.json"

for need in jq curl; do
  if ! command -v "$need" >/dev/null 2>&1; then
    echo "missing dependency: $need" >&2
    exit 1
  fi
done

for p in "$CRED_PATH" "$INSTALL_PATH"; do
  if [[ ! -f "$p" ]]; then
    echo "missing $p" >&2
    echo "run \`claude auth login\` on this machine first." >&2
    echo "(both ${CRED_PATH} and ${INSTALL_PATH} are required — Claude" >&2
    echo "Code treats credentials.json alone as a fresh-install session" >&2
    echo "and re-prompts for login.)" >&2
    exit 1
  fi
done

if [[ ! -r "$TOKEN_SRC" ]]; then
  echo "cannot read bearer token from $TOKEN_SRC" >&2
  exit 1
fi
TOKEN="$(cat "$TOKEN_SRC")"
if [[ -z "$TOKEN" ]]; then
  echo "bearer token source is empty: $TOKEN_SRC" >&2
  exit 1
fi

BODY="$(jq -n \
  --rawfile c "$CRED_PATH" \
  --rawfile i "$INSTALL_PATH" \
  '{credentials_json: $c, claude_json: $i}')"

HTTP_OUT="$(mktemp)"
trap 'rm -f "$HTTP_OUT"' EXIT

CODE=$(curl -sS -o "$HTTP_OUT" -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  --data-binary "$BODY" \
  "${BASE_URL}/v1/auth/claude-credentials")

if [[ "$CODE" != "204" ]]; then
  echo "POST returned HTTP ${CODE}" >&2
  cat "$HTTP_OUT" >&2
  echo >&2
  exit 1
fi

# Confirm by reading status back.
STATUS=$(curl -sS \
  -H "Authorization: Bearer ${TOKEN}" \
  "${BASE_URL}/v1/auth/claude-credentials")

echo "staged. server says: ${STATUS}"
echo
echo "next: restart any running repo-VMs so they pick up the new"
echo "credentials. fresh VMs will get them automatically."
