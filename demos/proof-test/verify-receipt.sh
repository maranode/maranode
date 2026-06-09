#!/usr/bin/env bash
# the stranger side. depends only on openssl and sha256sum/shasum. it does NOT
# use maranode and never talks to the daemon. give it the published public key,
# the signed receipt body, the signature, and the artifacts. usage:
#   verify-receipt.sh <pubkey> <receipt.body.json> <receipt.sig> <prompt> <output> [model]
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/_lib.sh"
need_openssl

pub="$1"; body="$2"; sig="$3"; prompt="$4"; output="$5"; model="${6:-}"

ok=1
pass(){ printf '  %s[ PASS ]%s %s\n' "$C_GREEN" "$C_OFF" "$1"; }
fail(){ printf '  %s[ FAIL ]%s %s\n' "$C_RED" "$C_OFF" "$1"; ok=0; }

# 1. which key signed this, and do we hold that public key?
want_kid="$(json_get "$body" key_id)"
have_kid="$(key_id_of "$pub")"
[ -n "$have_kid" ] && [ "$want_kid" = "$have_kid" ] \
  && pass "key id matches the published key ($have_kid)" \
  || fail "key id mismatch (receipt $want_kid, key $have_kid)"

# 2. signature over the exact receipt-body bytes
sigbin="$(mktemp)"; openssl base64 -d -A -in "$sig" > "$sigbin" 2>/dev/null
if openssl pkeyutl -verify -pubin -inkey "$pub" -rawin -in "$body" -sigfile "$sigbin" >/dev/null 2>&1; then
  pass "signature valid — the record is authentic and unmodified"
else
  fail "signature invalid — the record was forged or edited"
fi
rm -f "$sigbin"

# 3. the prompt is the one that was signed
want_in="$(json_get "$body" input_sha256)"; have_in="$(sha256 "$prompt")"
[ "$want_in" = "$have_in" ] && pass "input matches the receipt" \
  || fail "input changed (receipt ${want_in:0:12}…, file ${have_in:0:12}…)"

# 4. the output is the one that was signed
want_out="$(json_get "$body" output_sha256)"; have_out="$(sha256 "$output")"
[ "$want_out" = "$have_out" ] && pass "output matches the receipt" \
  || fail "output altered after signing (receipt ${want_out:0:12}…, file ${have_out:0:12}…)"

# 5. the model, if it is at hand (optional — it can be large or absent)
if [ -n "$model" ] && [ -f "$model" ]; then
  want_m="$(json_get "$body" model_sha256)"; have_m="$(sha256 "$model")"
  [ "$want_m" = "$have_m" ] && pass "model matches the receipt" \
    || fail "model differs from the receipt"
fi

echo
if [ "$ok" = 1 ]; then
  printf '%sVERIFIED%s — this output came from the named model under the recorded\n' "$C_GREEN$C_BOLD" "$C_OFF"
  printf 'settings, and nothing has changed since it was signed.\n'
  exit 0
else
  printf '%sNOT VERIFIED%s — do not trust this record.\n' "$C_RED$C_BOLD" "$C_OFF"
  exit 1
fi
