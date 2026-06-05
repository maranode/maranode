# Development guide

Main developer: [ondercsn](https://github.com/ondercsn).

## Prerequisites

| Tool | Install |
|------|---------|
| Rust 1.88+ | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| CMake 3.14+ | `brew install cmake` (macOS) · `apt install cmake` (Linux) |
| Xcode CLT | `xcode-select --install` (macOS only) |

## Build & run

```bash
# Auto-detect GPU and build (recommended)
make build

# Or explicitly choose a backend:
make build-cpu     # CPU only - always works
make build-metal   # Apple Metal (macOS)
make build-cuda    # NVIDIA CUDA
make build-rocm    # AMD ROCm
make build-npu     # Intel NPU / iGPU via OpenVINO (Intel Core Ultra)

# Run the daemon (isolation auto-disabled on macOS)
./target/release/maranoded

# With RAG enabled
./target/release/maranoded --rag
```

The daemon listens on `http://127.0.0.1:11984` by default. Open `/ui` in a browser.

## GPU acceleration

The build system defaults to CPU-only. Enable a GPU backend by choosing the right Makefile target or passing `--features` to cargo:

| Backend | Platform | Prerequisite | Build command |
|---------|----------|--------------|---------------|
| **Metal** | macOS (Apple Silicon / AMD) | Xcode CLT | `make build-metal` |
| **CUDA** | Linux / Windows | [CUDA Toolkit ≥ 11.8](https://developer.nvidia.com/cuda-downloads) | `make build-cuda` |
| **ROCm** | Linux | [ROCm ≥ 5.6](https://rocm.docs.amd.com/) | `make build-rocm` |
| **Vulkan** | Cross-platform | [Vulkan SDK](https://vulkan.lunarg.com/) | `make build-vulkan` |
| **OpenVINO (NPU/iGPU)** | Linux (Intel Core Ultra) | [OpenVINO Runtime](https://docs.openvino.ai/install) | `make build-npu` |
| **Ryzen AI (NPU)** | Linux/Windows (AMD Ryzen AI) | [AMD Ryzen AI SDK](https://ryzenai.docs.amd.com) | `make build-ryzenai` |
| **CPU** | Any | None | `make build-cpu` |

`make build` auto-detects your platform and picks the right backend automatically.

If a GPU/NPU SDK is present but the matching feature is not enabled, the build will print a `cargo:warning` telling you exactly which flag to add.

### OpenVINO / Intel NPU notes

`make build-npu` sets `LLAMA_CMAKE_ARGS="-DGGML_OPENVINO=ON"` automatically. Because `llama-cpp-sys-2` doesn't expose an `openvino` cargo feature, the cmake flag must be present on the **first** build - run `cargo clean` before switching from another backend.

The OpenVINO runtime must be sourced before starting the daemon:

```bash
source /opt/intel/openvino/setupvars.sh
./target/release/maranoded --device npu
```

If the runtime is not found on the `LD_LIBRARY_PATH`, the daemon will exit immediately with an actionable error rather than silently falling back to CPU.

### AMD Ryzen AI / XDNA NPU notes

`make build-ryzenai` sets `LLAMA_CMAKE_ARGS="-DGGML_RYZEN_AI=ON"` automatically. Same `cargo clean` rule applies when switching backends.

Install the [AMD Ryzen AI SDK](https://ryzenai.docs.amd.com) first, then set `RYZENAI_INSTALL_PATH` before running the daemon:

```bash
export RYZENAI_INSTALL_PATH=/opt/amd/ryzenai
./target/release/maranoded --device ryzenai
```

The daemon validates the runtime (`libaie_controller.so`) at startup and fails fast if it is not found.

To verify which device is active after starting the daemon, check the startup log or call `GET /health`:
```bash
curl http://localhost:11984/health | jq .device
```

## Importing models

```bash
# Language model (LLM)
./target/release/maranode model import /path/to/model.gguf --name llama3.2 --tag 3b

# Embedding model for RAG (must pass --type embedding)
./target/release/maranode model import /path/to/bge-m3.gguf --name bge-m3 --tag latest --type embedding
```

The `--type` flag determines where the model appears: LLM models show in the chat selector; embedding models appear only in the Models page and are used by RAG.

## Common CLI flags

| Flag | Default | Purpose |
|------|---------|---------|
| `--bind` | `127.0.0.1:11984` | Listen address |
| `--data-dir` | `~/Library/Application Support/maranode` | Models, DB, audit log |
| `--device` | `auto` | `auto` · `cpu` · `gpu` · `npu` |
| `--rag` | off | Enable local RAG |
| `--admin-key` | none (open dev mode) | Bearer token for protected endpoints |
| `--log-level` | `info` | `debug` · `info` · `warn` |

All flags can also be set via environment variables (`MARANODE_BIND`, `MARANODE_DEVICE`, etc.) or in a `config.toml` file. See `docs/config.toml.example`.

## Benchmarking

Build and run the benchmark tool:

```bash
make bench          # CPU
make bench-metal    # Apple Metal
make bench-cuda     # NVIDIA CUDA

./target/release/maranode-bench --model /path/to/model.gguf
```

Key flags:

| Flag | Default | Purpose |
|------|---------|---------|
| `--device` | `auto` | `auto` · `cpu` · `gpu` · `npu` |
| `--runs` / `-n` | `10` | Measurement runs |
| `--warmup` | `3` | Warmup runs (discarded) |
| `--max-tokens` | `128` | Tokens to generate per run |
| `--output` / `-o` | `table` | `table` or `json` |
| `--save <file>` | - | Save summary JSON for later comparison |
| `--compare <file>` | - | Diff current run against a saved baseline |

**CPU vs GPU comparison workflow:**

```bash
# 1. Baseline on CPU
./target/release/maranode-bench --model model.gguf --device cpu --save cpu.json

# 2. Re-run on GPU and compare
./target/release/maranode-bench --model model.gguf --device gpu --compare cpu.json
```

The comparison table highlights improvements in green and regressions in red.

## Run tests

```bash
cargo test --workspace
```

## Auto-restart on changes

```bash
cargo install cargo-watch   # one-time
cargo watch -x 'run --bin maranoded'
```

## Troubleshooting

**`linker 'cc' not found`** -> `xcode-select --install`

**Port 11984 in use (Ollama)** -> `./target/release/maranoded --bind 127.0.0.1:11435`

**First build is slow** -> normal; all deps compile from source. Install `sccache` (`brew install sccache` + `export RUSTC_WRAPPER=sccache`) to cache between rebuilds.
