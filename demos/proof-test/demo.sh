#!/usr/bin/env bash
# Proof-test, end to end, in one run
# Produce a signed receipt for an inference, verify it as a stranger using only
# openssl + sha256sum, then watch tampering get caught. Pass --replay for the
# reproducible-inference bonus beat
HERE="$(cd "$(dirname "$0")" && pwd)"
. "$HERE/_lib.sh"
need_openssl

mode="${1:-}"
work="$HERE/work"
rm -rf "$work"; mkdir -p "$work"

pause(){ sleep "${DEMO_PAUSE:-1}"; }
banner(){ printf '\n%s━━ %s ━━━━━━━━━━━━━━━━━━━━━━━━━━━%s\n' "$C_BLUE$C_BOLD" "$1" "$C_OFF"; }
run(){ printf '%s$ %s%s\n' "$C_DIM" "$*" "$C_OFF"; eval "$*"; }

banner "1. one-time setup: the daemon's signing identity"
run "openssl genpkey -algorithm ed25519 -out '$work/daemon.key' 2>/dev/null"
run "openssl pkey -in '$work/daemon.key' -pubout -out '$work/daemon.pub' 2>/dev/null"
kid="$(key_id_of "$work/daemon.pub")"
printf 'published public key id: %s%s%s   the verifier needs only this\n' "$C_BOLD" "$kid" "$C_OFF"
pause

banner "2. a model and a prompt"
printf 'MARANODE-DEMO placeholder for a real GGUF model file, v1\n' > "$work/model.gguf"
printf 'List the three medications in the attached discharge summary.' > "$work/prompt.txt"
printf 'model:  llama3.2:3b   sha256 %s…\n' "$(sha256 "$work/model.gguf" | cut -c1-12)"
printf 'prompt: %s\n' "$(cat "$work/prompt.txt")"
pause

banner "3. run the inference and emit a signed receipt"
run "'$HERE/run-inference.sh' '$work/prompt.txt' '$work/output.txt'"
printf '%s--- model output ---%s\n%s\n%s--------------------%s\n' "$C_DIM" "$C_OFF" "$(cat "$work/output.txt")" "$C_DIM" "$C_OFF"
run "'$HERE/sign-receipt.sh' '$work' '$work/model.gguf' '$work/prompt.txt' '$work/output.txt' '$work/daemon.key' '$work/daemon.pub'"
orig_out_h="$(json_get "$work/receipt.body.json" output_sha256)"
printf '%s--- receipt.json ---%s\n' "$C_DIM" "$C_OFF"; cat "$work/receipt.json"
pause

banner "4. verify as a stranger — no maranode, only openssl + sha256sum"
run "'$HERE/verify-receipt.sh' '$work/daemon.pub' '$work/receipt.body.json' '$work/receipt.sig' '$work/prompt.txt' '$work/output.txt' '$work/model.gguf'" || true
pause

if [ "$mode" = "--replay" ]; then
  banner "5. bonus: reproducible inference — re-run and compare bytes"
  run "'$HERE/run-inference.sh' '$work/prompt.txt' '$work/output.replay.txt'"
  replay_h="$(sha256 "$work/output.replay.txt")"
  if [ "$replay_h" = "$orig_out_h" ]; then
    printf '  %s[ PASS ]%s re-run is byte-identical (%s…) — the decision can be replayed\n' "$C_GREEN" "$C_OFF" "${replay_h:0:12}"
  else
    printf '  %s[ FAIL ]%s re-run differs\n' "$C_RED" "$C_OFF"
  fi
  pause
fi

banner "6. tamper: change one line of the output after signing"
run "sed -i.bak 's/Lisinopril 10 mg/Lisinopril 40 mg/' '$work/output.txt'"
printf 'someone edited the dose in the stored answer. re-verify:\n'
run "'$HERE/verify-receipt.sh' '$work/daemon.pub' '$work/receipt.body.json' '$work/receipt.sig' '$work/prompt.txt' '$work/output.txt' '$work/model.gguf'" || true
printf '%scaught:%s the signature still checks out, but the output no longer matches its hash.\n' "$C_BOLD" "$C_OFF"
pause

banner "7. cover-up: rewrite the receipt to match the edit"
newh="$(sha256 "$work/output.txt")"
run "sed -i.bak2 -E 's/\"output_sha256\":\"[a-f0-9]+\"/\"output_sha256\":\"$newh\"/' '$work/receipt.body.json'"
printf 'the attacker fixed the hash, but cannot re-sign without the private key. re-verify:\n'
run "'$HERE/verify-receipt.sh' '$work/daemon.pub' '$work/receipt.body.json' '$work/receipt.sig' '$work/prompt.txt' '$work/output.txt' '$work/model.gguf'" || true
printf '%scaught:%s editing the receipt breaks the signature.\n' "$C_BOLD" "$C_OFF"
pause

banner "done"
printf 'A stranger verified, offline, that a named model produced a specific answer,\n'
printf 'and every change to either was caught — using only standard tools.\n'
printf '%sOllama gives you tokens. This gives you tokens you can prove.%s\n' "$C_BOLD" "$C_OFF"
