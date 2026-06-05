#
# Maranode - Multi-stage Dockerfile (CPU)
#
# Stages:
#   1. builder  - Rust + C++ build environment (Debian Bookworm)
#   2. runtime  - Minimal Debian Slim runtime image
#
# Why glibc (not musl)?
#   llama.cpp is written in C++17 and links against the C++ standard library.
#   Building it against musl requires a full libc++ toolchain that is not
#   available in the standard Rust musl images.  Debian Bookworm + glibc gives
#   us g++, libstdc++, and OpenMP (for multi-core inference) out of the box.
#
# Build:
#   docker build -t maranode/runtime:latest .
#
# Run:
#   docker run -d \
#     --name maranode \
#     --cap-add NET_ADMIN \
#     -v maranode-data:/var/lib/maranode \
#     maranode/runtime:latest
#
#Stage 1: Builder
FROM rust:1.88-slim-bookworm AS builder

# TARGETARCH is injected by BuildKit (amd64 | arm64).
ARG TARGETARCH

# Build-time dependencies:
#   - cmake       → llama.cpp build system
#   - g++         → C++17 compiler (llama.cpp)
#   - clang / libclang-dev → required by bindgen to generate llama.cpp FFI bindings
#   - libgomp-dev → OpenMP for multi-threaded CPU inference
#   - pkg-config  → C library detection
#   - ca-certificates → HTTPS during cargo fetch
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake \
        make \
        g++ \
        clang \
        libclang-dev \
        libgomp1 \
        pkg-config \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set glibc native target per platform.
# Docker BuildKit runs each platform's builder natively (or via QEMU), so the
# default `cargo build` target is always correct - we only write it out for
# clarity in the COPY step below.
RUN case "${TARGETARCH}" in \
        amd64) echo "x86_64-unknown-linux-gnu"  > /etc/rust_target ;; \
        arm64) echo "aarch64-unknown-linux-gnu" > /etc/rust_target ;; \
        *)     echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac

WORKDIR /build

# Dependency caching layer
# Copy manifests first - cargo fetches & compiles deps in a cacheable layer.
COPY Cargo.toml Cargo.lock ./

# Stub sources so `cargo build` resolves dep graphs without real code.
RUN for crate in \
        maranode-common maranode-audit maranode-isolation maranode-store \
        maranode-inference maranode-api; do \
      mkdir -p crates/${crate}/src && \
      printf '// stub\n' > crates/${crate}/src/lib.rs; \
    done && \
    for bin in maranode-daemon maranode-cli; do \
      mkdir -p crates/${bin}/src && \
      printf 'fn main(){}\n' > crates/${bin}/src/main.rs; \
    done && \
    mkdir -p tests/integration tests/e2e && \
    printf '// stub\n' > tests/integration/stub.rs && \
    printf '// stub\n' > tests/e2e/stub.rs

COPY crates/maranode-common/Cargo.toml    crates/maranode-common/
COPY crates/maranode-audit/Cargo.toml     crates/maranode-audit/
COPY crates/maranode-isolation/Cargo.toml crates/maranode-isolation/
COPY crates/maranode-store/Cargo.toml     crates/maranode-store/
COPY crates/maranode-inference/Cargo.toml crates/maranode-inference/
COPY crates/maranode-api/Cargo.toml       crates/maranode-api/
COPY crates/maranode-daemon/Cargo.toml    crates/maranode-daemon/
COPY crates/maranode-cli/Cargo.toml       crates/maranode-cli/
COPY tests/Cargo.toml                   tests/

# This compiles all deps (including llama.cpp C++ sources) into a cached layer.
# The `|| true` ignores errors from the stub sources.
RUN cargo build --release --bin maranoded --bin maranode || true

# real build
COPY crates/ crates/
COPY tests/  tests/

# Touch source files so cargo sees them as newer than the stub build.
RUN find crates -name "*.rs" -exec touch {} +

RUN cargo build --release --bin maranoded --bin maranode \
    && RUST_TARGET=$(cat /etc/rust_target) \
    && mkdir -p /build/out \
    && cp "target/release/maranoded" "target/release/maranode" /build/out/

# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime

# Runtime dependencies:
#   - ca-certificates → HTTPS for model download in whitelist mode
#   - iptables        → air-gap enforcement (OUTPUT/INPUT DROP)
#   - ip6tables       → IPv6 air-gap enforcement (bundled with iptables on ARM64)
#   - libcap2-bin     → drop capabilities after applying firewall rules
#   - tini            → proper PID 1 / signal handling
#   - wget            → healthcheck (used by docker-compose healthcheck)
#   - libgomp1        → OpenMP runtime for multi-threaded llama.cpp inference
#   - libstdc++6      → C++ standard library linked by llama.cpp
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        iptables \
        libcap2-bin \
        tini \
        wget \
        libgomp1 \
        libstdc++6 \
    && rm -rf /var/lib/apt/lists/*

# Dedicated non-root user.
# NET_ADMIN capability is granted via `docker run --cap-add NET_ADMIN`
# (see docker-compose.yml) - not baked into the image.
RUN groupadd -r maranode && useradd -r -g maranode maranode

# Data directory.
RUN mkdir -p /var/lib/maranode && chown maranode:maranode /var/lib/maranode

# Copy binaries from builder.
COPY --from=builder /build/out/maranoded /build/out/maranode /usr/local/bin/

# Copy entrypoint script.
COPY docker/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

# Persistent data volume.
VOLUME ["/var/lib/maranode"]

# API port.
EXPOSE 11984

# Metadata.
LABEL org.opencontainers.image.title="Maranode Runtime" \
      org.opencontainers.image.description="Privacy-first AI runtime with provable network isolation" \
      org.opencontainers.image.source="https://github.com/maranode/maranode" \
      org.opencontainers.image.authors="ondercsn" \
      org.opencontainers.image.licenses="Apache-2.0"

# tini as PID 1 → correct signal forwarding to maranoded.
ENTRYPOINT ["/usr/bin/tini", "--", "/entrypoint.sh"]
CMD ["--air-gap"]
