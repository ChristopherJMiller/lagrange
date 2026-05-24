#!/usr/bin/env bash
# Stage a GitHub fine-grained PAT into the lagrange admin service. Each
# repo-VM picks the token up at create/restart time and configures git's
# credential helper to use it, so agents inside the VM can `git push`
# to the authorized repos.
#
# Create the PAT at https://github.com/settings/personal-access-tokens
# with these permissions:
#   - Repository access:    select the repos you want lagrange to push to
#   - Contents:             Read and write
#   - Pull requests:        Read and write   (optional; for opening PRs)
#   - Workflows:            Read and write   (optional; for changing CI)
#
# Usage:
#   scripts/stage-github-token.sh <base-url> <bearer-token-source> <pat-source>
#
# Examples:
#   # Inline PAT, via SSH-fetched bearer:
#   scripts/stage-github-token.sh \
#     http://10.99.0.2:8443 \
#     <(ssh chris@<host> 'sudo cat /run/secrets/admin-service-token') \
#     <(echo "$GITHUB_PAT")
#
#   # PAT from a local file:
#   scripts/stage-github-token.sh http://10.99.0.2:8443 ./bearer.txt ./gh-pat.txt

set -euo pipefail

if [[ $# -ne 3 ]]; then
  cat >&2 <<EOF
Usage: $0 <base-url> <bearer-token-source> <pat-source>

  base-url             e.g. http://lagrange-admin.ops.svc:8443
                          or http://10.99.0.2:8443
  bearer-token-source  path to a file containing the admin-service bearer
                          token. Use process substitution to source from a
                          command: <(ssh ... 'sudo cat /run/secrets/...')
  pat-source           path to a file containing the fine-grained PAT.
                          Use process substitution to source from an env
                          var: <(echo "\$GITHUB_PAT")
EOF
  exit 2
fi

BASE_URL="$1"
TOKEN_SRC="$2"
PAT_SRC="$3"

for need in jq curl; do
  if ! command -v "$need" >/dev/null 2>&1; then
    echo "missing dependency: $need" >&2
    exit 1
  fi
done

for path in "$TOKEN_SRC" "$PAT_SRC"; do
  if [[ ! -r "$path" ]]; then
    echo "cannot read $path" >&2
    exit 1
  fi
done

TOKEN="$(cat "$TOKEN_SRC")"
PAT="$(cat "$PAT_SRC" | tr -d '[:space:]')"

if [[ -z "$TOKEN" || -z "$PAT" ]]; then
  echo "bearer token or PAT is empty" >&2
  exit 1
fi

BODY="$(jq -n --arg t "$PAT" '{token: $t}')"

HTTP_OUT="$(mktemp)"
trap 'rm -f "$HTTP_OUT"' EXIT

CODE=$(curl -sS -o "$HTTP_OUT" -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  --data-binary "$BODY" \
  "${BASE_URL}/v1/auth/github-token")

if [[ "$CODE" != "204" ]]; then
  echo "POST returned HTTP ${CODE}" >&2
  cat "$HTTP_OUT" >&2
  echo >&2
  exit 1
fi

STATUS=$(curl -sS \
  -H "Authorization: Bearer ${TOKEN}" \
  "${BASE_URL}/v1/auth/github-token")
echo "staged. server says: ${STATUS}"
echo
echo "next: any running repo-VMs need a restart to pick up the new"
echo "token. fresh VMs get it automatically; agents can \`git push\`"
echo "to repos this PAT has write access to."
