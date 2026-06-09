# shared helpers for the proof-test demo
# no set -e here on purpose: the verifier wants to run every check and report,
# not abort on the first failing one.
set -uo pipefail

if [ -t 1 ]; then
  C_GREEN=$'\033[0;32m'; C_RED=$'\033[0;31m'; C_BLUE=$'\033[0;34m'
  C_DIM=$'\033[2m'; C_BOLD=$'\033[1m'; C_OFF=$'\033[0m'
else
  C_GREEN=; C_RED=; C_BLUE=; C_DIM=; C_BOLD=; C_OFF=
fi

# sha256 of a file, hex only. works with sha256sum or shasum.
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
  sha256_stdin() { sha256sum | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
  sha256_stdin() { shasum -a 256 | cut -d' ' -f1; }
else
  echo "need sha256sum or shasum" >&2; exit 1
fi

need_openssl() {
  command -v openssl >/dev/null 2>&1 || { echo "openssl not found" >&2; exit 1; }
  case "$(openssl version 2>/dev/null)" in
    LibreSSL*)
      printf '%swarning:%s LibreSSL may not sign ed25519 one-shot. On macOS install openssl (brew install openssl) and put it first on PATH.\n' \
        "${C_RED:-}" "${C_OFF:-}" >&2 ;;
  esac
}

# key id: first 16 hex of sha256 over the DER public key
key_id_of() { openssl pkey -pubin -in "$1" -outform DER 2>/dev/null | sha256_stdin | cut -c1-16; }

# read one field from compact json
json_get() {
  grep -oE "\"$2\":(\"[^\"]*\"|[0-9]+)" "$1" 2>/dev/null | head -n1 | sed -E "s/\"$2\"://; s/^\"//; s/\"$//" || true
}
