# Reproducible Inference — Conditions and Limits

Maranode can re-run an inference and produce bit-exact output under specific
conditions. This document explains what "reproducible" means here, when it holds,
and when it does not.

---

## What is needed for exact reproduction

Four things must match between the original run and the replay:

1. **Greedy decoding** — `deterministic: true` in the request (or `--deterministic`
   on the CLI). This pins temperature to 0, top-k to 1, and seed to 0. Any
   stochastic sampling breaks bit-exact reproduction.

2. **Same model file** — the GGUF file must be byte-for-byte identical. The
   receipt records `model_sha256` for this check.

3. **Deterministic kernels build** — the daemon must be compiled with the
   `deterministic-kernels` feature:

   ```
   cargo build --features deterministic-kernels
   ```

   This passes `-DGGML_DETERMINISTIC=ON` to the llama.cpp cmake build, which
   fixes the floating-point reduction order in RMSNorm, MatMul, and attention.
   Without this flag, thread scheduling may produce different rounding across runs
   even with identical input.

4. **Same hardware class** — the CPU or GPU architecture must be the same. The
   x86-64 AVX2 path, the ARM NEON path, and any GPU path each have different
   floating-point unit behaviour. A run on an Apple M-series chip will not
   reproduce on an Intel Core machine, and vice versa.

The receipt records `env.kernel_build_id` and `env.device_class` so you can check
these conditions offline.

---

## What `maranode audit replay` checks

The command re-runs the original inference via the running daemon (requires
`log_prompts=true` in daemon config so the original messages are available), then
compares `output_sha256` from the new receipt against the stored one.

It does not verify kernel build or hardware class automatically. If the hashes
differ, the most common causes are:

- Different hardware between the original run and the replay node
- Daemon not built with `deterministic-kernels`
- Model file replaced or re-quantized since the original run

---

## Known limits

**Greedy only.** Any non-zero temperature breaks reproducibility. The receipt
records `decode_params.deterministic` so third parties can confirm greedy mode was
used. Stochastic runs are not reproducible and this is expected.

**Same hardware class.** We do not claim cross-architecture reproduction. The
`env.device_class` and `env.kernel_build_id` fields in the receipt let a verifier
confirm the hardware class. A verifier on different hardware can still check the
signature and the model hash; it just cannot re-derive the exact output bytes.

**No cross-quantization guarantee.** A q4_0 model and a q8_0 model of the same
base weights will produce different outputs. The `model_sha256` and `model_quant`
fields make this explicit.

**Floating-point variance on GPU.** Even with `GGML_DETERMINISTIC=ON`, GPU paths
(CUDA, Metal, ROCm) may still have non-deterministic reductions depending on driver
version and GPU model. The `deterministic-kernels` feature is most reliable on CPU.
For GPU, test on your specific hardware before relying on byte-exact replay.

**Context window.** The prompt token count must fit inside the model context window.
If a system prompt is added between the original run and the replay (for example by
changing the workspace system prompt), the effective input changes and hashes will
differ.

---

## How to confirm deterministic builds in the receipt

Check the `env.kernel_build_id` field of the receipt:

```
"env": {
  "kernel_build_id": "llama-cpp-2@0.1.146+deterministic",
  "thread_count": 8,
  "device_class": "cpu"
}
```

The `+deterministic` suffix means `GGML_DETERMINISTIC=ON` was active at build
time. A receipt without this suffix was produced by a binary that cannot guarantee
bit-exact replay.

---

## Summary table

| Condition | Reproducible? |
|---|---|
| Same node, greedy, `deterministic-kernels`, same model | Yes |
| Same node, greedy, no `deterministic-kernels` | Usually, not guaranteed |
| Same hardware class, greedy, `deterministic-kernels` | Yes |
| Different x86 vs ARM | No |
| GPU vs CPU | No |
| Any non-zero temperature | No |
| Different model file / quant | No |
