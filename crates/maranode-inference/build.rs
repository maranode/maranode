fn main() {
    let metal_on = std::env::var("CARGO_FEATURE_METAL").is_ok();
    let cuda_on = std::env::var("CARGO_FEATURE_CUDA").is_ok();
    let rocm_on = std::env::var("CARGO_FEATURE_ROCM").is_ok();
    let vulkan_on = std::env::var("CARGO_FEATURE_VULKAN").is_ok();
    let openvino_on = std::env::var("CARGO_FEATURE_OPENVINO").is_ok();
    let ryzenai_on = std::env::var("CARGO_FEATURE_RYZENAI").is_ok();
    let any_gpu_on = metal_on || cuda_on || rocm_on || vulkan_on || openvino_on || ryzenai_on;

    // on macos, warn when metal feature is not enabled
    if cfg!(target_os = "macos") && !metal_on {
        println!(
            "cargo:warning=Building on macOS without Metal: CPU inference only. \
             For GPU acceleration run: cargo build --features metal \
             (or: make build-metal)"
        );
    }

    // on linux/windows, look for cuda install
    if !cfg!(target_os = "macos") && !cuda_on {
        let cuda_found = std::env::var("CUDA_PATH").is_ok()
            || std::env::var("CUDA_HOME").is_ok()
            || std::path::Path::new("/usr/local/cuda").exists()
            || which("nvcc");

        if cuda_found {
            println!(
                "cargo:warning=CUDA toolkit detected but the `cuda` feature is not enabled: \
                 running CPU inference only. For GPU acceleration run: \
                 cargo build --features cuda  (or: make build-cuda)"
            );
        }
    }

    // on linux, look for rocm install
    if cfg!(target_os = "linux") && !rocm_on && !cuda_on {
        let rocm_found = std::env::var("ROCM_PATH").is_ok()
            || std::path::Path::new("/opt/rocm").exists()
            || which("hipcc");

        if rocm_found {
            println!(
                "cargo:warning=ROCm SDK detected but the `rocm` feature is not enabled: \
                 running CPU inference only. For AMD GPU acceleration run: \
                 cargo build --features rocm  (or: make build-rocm)"
            );
        }
    }

    // on linux, look for openvino / npu
    if cfg!(target_os = "linux") && !openvino_on {
        let ov_found = std::env::var("OPENVINO_DIR").is_ok()
            || std::path::Path::new("/opt/intel/openvino").exists()
            || std::path::Path::new("/opt/intel/openvino_2024").exists()
            || std::path::Path::new("/opt/intel/openvino_2023").exists();

        if ov_found {
            println!(
                "cargo:warning=OpenVINO installation detected but the `openvino` feature is not \
                 enabled: NPU/iGPU inference unavailable. To enable: \
                 LLAMA_CMAKE_ARGS=\"-DGGML_OPENVINO=ON\" cargo build --features openvino"
            );
        }
    }

    // on linux, look for amd ryzen ai sdk
    if cfg!(target_os = "linux") && !ryzenai_on {
        let ryzenai_found = std::env::var("RYZENAI_INSTALL_PATH").is_ok()
            || std::path::Path::new("/opt/amd/ryzenai").exists()
            || std::path::Path::new("/opt/xilinx/xrt").exists();

        if ryzenai_found {
            println!(
                "cargo:warning=AMD Ryzen AI SDK detected but the `ryzenai` feature is not \
                 enabled: NPU inference unavailable. To enable: \
                 LLAMA_CMAKE_ARGS=\"-DGGML_RYZEN_AI=ON\" cargo build --features ryzenai"
            );
        }
    }

    if openvino_on {
        configure_openvino();
    }

    if ryzenai_on {
        configure_ryzenai();
    }

    if any_gpu_on {
        println!("cargo:rustc-cfg=gpu_enabled");
    }

    println!("cargo:rerun-if-env-changed=CUDA_PATH");
    println!("cargo:rerun-if-env-changed=CUDA_HOME");
    println!("cargo:rerun-if-env-changed=ROCM_PATH");
    println!("cargo:rerun-if-env-changed=OPENVINO_DIR");
    println!("cargo:rerun-if-env-changed=RYZENAI_INSTALL_PATH");
    println!("cargo:rerun-if-env-changed=LLAMA_CMAKE_ARGS");
}

fn configure_openvino() {
    // common openvino lib paths, try newer versions first
    let candidates: &[&str] = &[
        "/opt/intel/openvino_2024/runtime/lib/intel64",
        "/opt/intel/openvino_2023/runtime/lib/intel64",
        "/opt/intel/openvino/runtime/lib/intel64",
    ];

    let env_dir = std::env::var("OPENVINO_DIR").ok();

    let lib_dir = env_dir
        .as_deref()
        .map(|d| std::path::PathBuf::from(d).join("runtime/lib/intel64"))
        .into_iter()
        .chain(candidates.iter().map(std::path::PathBuf::from))
        .find(|p| p.exists());

    match lib_dir {
        Some(dir) => {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=dylib=openvino_c");
            println!("cargo:rustc-cfg=openvino_runtime_found");
        }
        None => {
            println!(
                "cargo:warning=OpenVINO runtime not found in standard paths and OPENVINO_DIR is \
                 not set. NPU inference will fail at runtime. \
                 Install OpenVINO from https://docs.openvino.ai and set OPENVINO_DIR."
            );
        }
    }

    // llama-cpp-sys-2 v0.1.x has no openvino cargo feature, you must set cmake flag
    // before first build, for example:
    //
    //   LLAMA_CMAKE_ARGS="-DGGML_OPENVINO=ON" cargo build --features openvino
    //
    // we warn when flag missing, because otherwise npu build looks ok but runs on cpu
    let cmake_args = std::env::var("LLAMA_CMAKE_ARGS").unwrap_or_default();
    if !cmake_args.contains("GGML_OPENVINO") {
        println!(
            "cargo:warning=LLAMA_CMAKE_ARGS does not contain -DGGML_OPENVINO=ON. \
             llama.cpp will be compiled WITHOUT OpenVINO support and NPU inference \
             will silently fall back to CPU. Re-run cargo clean then rebuild with: \
             LLAMA_CMAKE_ARGS=\"-DGGML_OPENVINO=ON\" cargo build --features openvino"
        );
    }
}

fn configure_ryzenai() {
    let candidates: &[&str] = &["/opt/amd/ryzenai/lib", "/opt/xilinx/xrt/lib"];

    let env_dir = std::env::var("RYZENAI_INSTALL_PATH").ok();

    let lib_dir = env_dir
        .as_deref()
        .map(|d| std::path::PathBuf::from(d).join("lib"))
        .into_iter()
        .chain(candidates.iter().map(std::path::PathBuf::from))
        .find(|p| p.exists());

    match lib_dir {
        Some(dir) => {
            println!("cargo:rustc-link-search=native={}", dir.display());
            println!("cargo:rustc-link-lib=dylib=aie_controller");
            println!("cargo:rustc-cfg=ryzenai_runtime_found");
        }
        None => {
            println!(
                "cargo:warning=AMD Ryzen AI runtime not found in standard paths and \
                 RYZENAI_INSTALL_PATH is not set. NPU inference will fail at runtime. \
                 Install the AMD Ryzen AI SDK from https://ryzenai.docs.amd.com"
            );
        }
    }

    let cmake_args = std::env::var("LLAMA_CMAKE_ARGS").unwrap_or_default();
    if !cmake_args.contains("GGML_RYZEN_AI") {
        println!(
            "cargo:warning=LLAMA_CMAKE_ARGS does not contain -DGGML_RYZEN_AI=ON. \
             llama.cpp will be compiled WITHOUT Ryzen AI support and NPU inference \
             will silently fall back to CPU. Re-run cargo clean then rebuild with: \
             LLAMA_CMAKE_ARGS=\"-DGGML_RYZEN_AI=ON\" cargo build --features ryzenai"
        );
    }
}

fn which(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
