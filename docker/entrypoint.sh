#!/bin/sh
# docker/entrypoint.sh
#
# Maranode container entrypoint.

set -e

DATA_DIR="${MARANODE_DATA_DIR:-/var/lib/maranode}"
BIND="${MARANODE_BIND:-0.0.0.0:11984}"
LOG_LEVEL="${MARANODE_LOG_LEVEL:-info}"

have_net_admin() {
    iptables -L INPUT -n >/dev/null 2>&1
}

ARGS="--data-dir ${DATA_DIR} --bind ${BIND} --log-level ${LOG_LEVEL}"

EXTRA_ARGS="$*"

if echo "${EXTRA_ARGS}" | grep -q -- "--no-isolation"; then
    ISOLATION_FLAG="--no-isolation"
elif [ "${MARANODE_NO_ISOLATION}" = "1" ]; then
    echo "[maranode] MARANODE_NO_ISOLATION=1 - skipping network isolation." >&2
    ISOLATION_FLAG="--no-isolation"
elif have_net_admin; then
    echo "[maranode] NET_ADMIN capability detected - air-gap mode active." >&2
    ISOLATION_FLAG="--air-gap"
else
    echo "[maranode] WARNING: NET_ADMIN capability not available." >&2
    echo "[maranode]   iptables rules cannot be applied." >&2
    echo "[maranode]   To enable air-gap mode, add: --cap-add NET_ADMIN" >&2
    echo "[maranode]   Falling back to --no-isolation (inference still works)." >&2
    ISOLATION_FLAG="--no-isolation"
fi

EXTRA_ARGS=$(echo "${EXTRA_ARGS}" | sed 's/--air-gap//g; s/--no-isolation//g')

echo "[maranode] Starting: maranoded ${ARGS} ${ISOLATION_FLAG} ${EXTRA_ARGS}" >&2

exec maranoded ${ARGS} ${ISOLATION_FLAG} ${EXTRA_ARGS}
