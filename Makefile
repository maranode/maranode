.PHONY: build build-cpu build-metal build-cuda build-rocm build-vulkan build-npu build-ryzenai \
        bench bench-metal bench-cuda \
        test check fmt clippy clean help


## cpu-only build (always works, no GPU SDK required)
build-cpu:
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features

## apple metal (macos - apple silicon and amd)
build-metal:
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features metal

## nvidia cuda (requires CUDA toolkit ≥ 11.8)
build-cuda:
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features cuda

## amd rocm / hip (requires ROCm ≥ 5.6)
build-rocm:
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features rocm

## vulkan (cross-platform, requires Vulkan SDK + drivers)
build-vulkan:
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features vulkan

## intel npu / igpu via openvino (intel core ultra; requires openvino runtime)
## source /opt/intel/openvino/setupvars.sh before running maranoded
build-npu:
	LLAMA_CMAKE_ARGS="-DGGML_OPENVINO=ON" \
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features openvino

## amd ryzen ai npu via xdna driver (requires amd ryzen ai sdk)
## set RYZENAI_INSTALL_PATH to the sdk root before running maranoded
build-ryzenai:
	LLAMA_CMAKE_ARGS="-DGGML_RYZEN_AI=ON" \
	cargo build --release \
	  --bin maranoded --bin maranode \
	  --no-default-features --features ryzenai

## auto-detect platform and build the right backend
build:
	@if [ "$$(uname)" = "Darwin" ]; then \
	  echo "macOS detected - building with Metal"; \
	  $(MAKE) build-metal; \
	elif command -v nvcc >/dev/null 2>&1 || [ -d /usr/local/cuda ]; then \
	  echo "CUDA detected - building with CUDA"; \
	  $(MAKE) build-cuda; \
	elif command -v hipcc >/dev/null 2>&1 || [ -d /opt/rocm ]; then \
	  echo "ROCm detected - building with ROCm"; \
	  $(MAKE) build-rocm; \
	elif [ -d /opt/intel/openvino ] || [ -n "$$OPENVINO_DIR" ]; then \
	  echo "OpenVINO detected - building with NPU/iGPU support"; \
	  $(MAKE) build-npu; \
	elif [ -d /opt/amd/ryzenai ] || [ -n "$$RYZENAI_INSTALL_PATH" ]; then \
	  echo "AMD Ryzen AI SDK detected - building with XDNA NPU support"; \
	  $(MAKE) build-ryzenai; \
	else \
	  echo "No GPU SDK detected - building CPU-only"; \
	  $(MAKE) build-cpu; \
	fi

## benchmark targets

bench:
	cargo build --release --bin maranode-bench --no-default-features
	@echo "Run: ./target/release/maranode-bench --model /path/to/model.gguf"

bench-metal:
	cargo build --release --bin maranode-bench --no-default-features --features metal
	@echo "Run: ./target/release/maranode-bench --model /path/to/model.gguf --device gpu"

bench-cuda:
	cargo build --release --bin maranode-bench --no-default-features --features cuda
	@echo "Run: ./target/release/maranode-bench --model /path/to/model.gguf --device gpu"

# dev targets

test:
	cargo test --workspace

check:
	cargo check --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace -- -D warnings

clean:
	cargo clean

help:
	@echo "Maranode build targets:"
	@echo ""
	@echo "  make build         Auto-detect GPU and build"
	@echo "  make build-cpu     CPU-only (no GPU SDK needed)"
	@echo "  make build-metal   Apple Metal (macOS)"
	@echo "  make build-cuda    NVIDIA CUDA"
	@echo "  make build-rocm    AMD ROCm"
	@echo "  make build-vulkan  Vulkan (cross-platform GPU)"
	@echo "  make build-npu     Intel NPU/iGPU via OpenVINO (Intel Core Ultra)"
	@echo "  make build-ryzenai AMD Ryzen AI NPU via XDNA driver"
	@echo ""
	@echo "  make bench         Build benchmark tool (CPU)"
	@echo "  make bench-metal   Build benchmark tool (Metal)"
	@echo "  make bench-cuda    Build benchmark tool (CUDA)"
	@echo ""
	@echo "  make test          Run all tests"
	@echo "  make check         Cargo check (fast)"
	@echo "  make clippy        Lint"
	@echo "  make fmt           Format"
	@echo "  make clean         Remove build artifacts"
