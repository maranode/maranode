#!/usr/bin/env bash
# the daemon side. build a receipt for one inference and sign it with the
# daemon key. usage:
#   sign-receipt.sh <workdir> <model> <prompt> <output> <privkey> <pubkey>
set -e
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/_lib.sh"
need_openssl

work="$1"; model="$2"; prompt="$3"; output="$4"; priv="$5"; pub="$6"

model_h="$(sha256 "$model")"
in_h="$(sha256 "$prompt")"
out_h="$(sha256 "$output")"
kid="$(key_id_of "$pub")"
ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# the signed payload
body="$work/receipt.body.json"
printf '{"version":1,"model_id":"llama3.2:3b","model_sha256":"%s","decode":"greedy","temperature":0,"top_k":1,"seed":42,"input_sha256":"%s","output_sha256":"%s","timestamp":"%s","key_id":"%s"}' \
  "$model_h" "$in_h" "$out_h" "$ts" "$kid" > "$body"

# ed25519 signature
openssl pkeyutl -sign -inkey "$priv" -rawin -in "$body" 2>/dev/null | openssl base64 -A > "$work/receipt.sig"

# a combined, human-facing receipt
printf '{\n  "body": %s,\n  "algorithm": "ed25519",\n  "public_key_id": "%s",\n  "signature_b64": "%s"\n}\n' \
  "$(cat "$body")" "$kid" "$(cat "$work/receipt.sig")" > "$work/receipt.json"
