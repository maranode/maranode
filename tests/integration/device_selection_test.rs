//! tests for device preference and which backends were compiled in

use maranode_common::models::InferenceDevice;
use maranode_inference::engine::InferenceEngine;
use maranode_inference::stub::StubEngine;
use maranode_inference::{DevicePreference, LlamaCppEngine};

#[test]
fn inference_device_display() {
    assert_eq!(InferenceDevice::Cpu.to_string(), "cpu");
    assert_eq!(InferenceDevice::Gpu.to_string(), "gpu");
    assert_eq!(InferenceDevice::Metal.to_string(), "metal");
    assert_eq!(InferenceDevice::Npu.to_string(), "npu");
    assert_eq!(InferenceDevice::RyzenAi.to_string(), "ryzenai");
}

#[test]
fn inference_device_serde_roundtrip() {
    let cases = [
        (InferenceDevice::Cpu, "\"cpu\""),
        (InferenceDevice::Gpu, "\"gpu\""),
        (InferenceDevice::Metal, "\"metal\""),
        (InferenceDevice::Npu, "\"npu\""),
        (InferenceDevice::RyzenAi, "\"ryzenai\""),
    ];
    for (device, expected_json) in cases {
        let json = serde_json::to_string(&device).unwrap();
        assert_eq!(json, expected_json, "serialize {device}");

        let back: InferenceDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, device, "deserialize {device}");
    }
}

#[test]
fn inference_device_is_accelerated() {
    assert!(
        !InferenceDevice::Cpu.is_accelerated(),
        "CPU is not accelerated"
    );
    assert!(InferenceDevice::Gpu.is_accelerated(), "GPU is accelerated");
    assert!(
        InferenceDevice::Metal.is_accelerated(),
        "Metal is accelerated"
    );
    assert!(InferenceDevice::Npu.is_accelerated(), "NPU is accelerated");
    assert!(
        InferenceDevice::RyzenAi.is_accelerated(),
        "RyzenAi is accelerated"
    );
}

#[test]
fn device_pref_cpu_always_succeeds() {
    // CPU preference always succeeds
    let engine = LlamaCppEngine::new(DevicePreference::Cpu)
        .expect("DevicePreference::Cpu must always succeed");

    assert_eq!(
        engine.device(),
        InferenceDevice::Cpu,
        "DevicePreference::Cpu must yield InferenceDevice::Cpu"
    );
}

#[test]
fn device_pref_cpu_reports_non_accelerated() {
    let engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
    assert!(
        !engine.device().is_accelerated(),
        "CPU device must not report as accelerated"
    );
}

#[test]
fn device_pref_auto_matches_compiled_feature() {
    let engine = LlamaCppEngine::new(DevicePreference::Auto)
        .expect("DevicePreference::Auto must always succeed");

    let device = engine.device();

    // Auto selects best backend that was compiled in
    // Order: metal, then cuda, rocm, vulkan, openvino, ryzenai, else cpu

    #[cfg(feature = "metal")]
    assert_eq!(
        device,
        InferenceDevice::Metal,
        "Auto on a Metal build must yield Metal"
    );

    #[cfg(all(feature = "cuda", not(feature = "metal")))]
    assert_eq!(
        device,
        InferenceDevice::Gpu,
        "Auto on a CUDA build must yield Gpu"
    );

    #[cfg(all(feature = "rocm", not(any(feature = "metal", feature = "cuda"))))]
    assert_eq!(
        device,
        InferenceDevice::Gpu,
        "Auto on a ROCm build must yield Gpu"
    );

    #[cfg(all(
        feature = "vulkan",
        not(any(feature = "metal", feature = "cuda", feature = "rocm"))
    ))]
    assert_eq!(
        device,
        InferenceDevice::Gpu,
        "Auto on a Vulkan build must yield Gpu"
    );

    #[cfg(all(
        feature = "openvino",
        not(any(
            feature = "metal",
            feature = "cuda",
            feature = "rocm",
            feature = "vulkan"
        ))
    ))]
    assert_eq!(
        device,
        InferenceDevice::Npu,
        "Auto on an OpenVINO-only build must yield Npu"
    );

    #[cfg(all(
        feature = "ryzenai",
        not(any(
            feature = "metal",
            feature = "cuda",
            feature = "rocm",
            feature = "vulkan",
            feature = "openvino"
        ))
    ))]
    assert_eq!(
        device,
        InferenceDevice::RyzenAi,
        "Auto on a Ryzen AI-only build must yield RyzenAi"
    );

    #[cfg(not(any(
        feature = "metal",
        feature = "cuda",
        feature = "rocm",
        feature = "vulkan",
        feature = "openvino",
        feature = "ryzenai"
    )))]
    assert_eq!(
        device,
        InferenceDevice::Cpu,
        "Auto on a CPU-only build must yield Cpu"
    );

    // Device enum can convert to string and JSON
    let _ = device.to_string();
    let _ = serde_json::to_string(&device).unwrap();
}

#[cfg(any(
    feature = "metal",
    feature = "cuda",
    feature = "rocm",
    feature = "vulkan"
))]
#[test]
fn device_pref_gpu_succeeds_when_gpu_compiled() {
    let engine = LlamaCppEngine::new(DevicePreference::Gpu)
        .expect("DevicePreference::Gpu must succeed when a GPU feature is compiled");

    assert!(
        engine.device().is_accelerated(),
        "DevicePreference::Gpu must yield an accelerated device"
    );
}

#[cfg(not(any(
    feature = "metal",
    feature = "cuda",
    feature = "rocm",
    feature = "vulkan"
)))]
#[test]
fn device_pref_gpu_fails_when_no_gpu_compiled() {
    let result = LlamaCppEngine::new(DevicePreference::Gpu);
    assert!(
        result.is_err(),
        "DevicePreference::Gpu must fail on a CPU/NPU-only build: got Ok"
    );

    let err = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err.contains("gpu")
            || err.contains("metal")
            || err.contains("cuda")
            || err.contains("compiled"),
        "error message should explain that no GPU backend is available: {err}"
    );
}

#[cfg(feature = "openvino")]
#[test]
fn device_pref_npu_succeeds_when_openvino_compiled() {
    let engine = LlamaCppEngine::new(DevicePreference::Npu)
        .expect("DevicePreference::Npu must succeed when openvino feature is compiled");

    assert_eq!(
        engine.device(),
        InferenceDevice::Npu,
        "DevicePreference::Npu must yield InferenceDevice::Npu"
    );
    assert!(
        engine.device().is_accelerated(),
        "Npu device must report as accelerated"
    );
}

#[cfg(not(feature = "openvino"))]
#[test]
fn device_pref_npu_fails_when_openvino_not_compiled() {
    let result = LlamaCppEngine::new(DevicePreference::Npu);
    assert!(
        result.is_err(),
        "DevicePreference::Npu must fail when openvino is not compiled: got Ok"
    );

    let err = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err.contains("npu") || err.contains("openvino") || err.contains("compiled"),
        "error message should explain that the OpenVINO backend is not available: {err}"
    );
}

#[cfg(feature = "metal")]
mod metal {
    use super::*;

    #[test]
    fn metal_auto_yields_metal() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(engine.device(), InferenceDevice::Metal);
    }

    #[test]
    fn metal_is_accelerated() {
        assert!(InferenceDevice::Metal.is_accelerated());
    }

    #[test]
    fn metal_cpu_override_works() {
        // Force CPU device even when Metal GPU was compiled
        let cpu_engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
        assert_eq!(cpu_engine.device(), InferenceDevice::Cpu);
        assert!(!cpu_engine.device().is_accelerated());
    }

    #[test]
    fn metal_device_serialises_as_metal() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        let json = serde_json::to_string(&engine.device()).unwrap();
        assert_eq!(
            json, "\"metal\"",
            "Metal device must serialise as 'metal' in API responses"
        );
    }
}

#[cfg(all(feature = "cuda", not(feature = "metal")))]
mod cuda {
    use super::*;

    #[test]
    fn cuda_auto_yields_gpu() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(engine.device(), InferenceDevice::Gpu);
    }

    #[test]
    fn cuda_is_accelerated() {
        let engine = LlamaCppEngine::new(DevicePreference::Gpu).unwrap();
        assert!(engine.device().is_accelerated());
    }

    #[test]
    fn cuda_device_serialises_as_gpu() {
        let engine = LlamaCppEngine::new(DevicePreference::Gpu).unwrap();
        let json = serde_json::to_string(&engine.device()).unwrap();
        assert_eq!(
            json, "\"gpu\"",
            "CUDA device must serialise as 'gpu' in API responses"
        );
    }
}

#[cfg(all(feature = "rocm", not(any(feature = "metal", feature = "cuda"))))]
mod rocm {
    use super::*;

    #[test]
    fn rocm_auto_yields_gpu() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Gpu,
            "ROCm build: Auto must yield Gpu"
        );
    }

    #[test]
    fn rocm_gpu_pref_succeeds() {
        let engine = LlamaCppEngine::new(DevicePreference::Gpu).unwrap();
        assert!(
            engine.device().is_accelerated(),
            "ROCm: DevicePreference::Gpu must yield an accelerated device"
        );
    }

    #[test]
    fn rocm_cpu_override_works() {
        let engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Cpu,
            "ROCm build: CPU override must work"
        );
    }

    #[test]
    fn rocm_device_serialises_as_gpu() {
        let engine = LlamaCppEngine::new(DevicePreference::Gpu).unwrap();
        let json = serde_json::to_string(&engine.device()).unwrap();
        assert_eq!(
            json, "\"gpu\"",
            "ROCm device must serialise as 'gpu' in API responses"
        );
    }
}

#[cfg(all(
    feature = "vulkan",
    not(any(feature = "metal", feature = "cuda", feature = "rocm"))
))]
mod vulkan {
    use super::*;

    #[test]
    fn vulkan_auto_yields_gpu() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Gpu,
            "Vulkan build: Auto must yield Gpu"
        );
    }

    #[test]
    fn vulkan_gpu_pref_succeeds() {
        let engine = LlamaCppEngine::new(DevicePreference::Gpu).unwrap();
        assert!(engine.device().is_accelerated());
    }

    #[test]
    fn vulkan_cpu_override_works() {
        let engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
        assert_eq!(engine.device(), InferenceDevice::Cpu);
        assert!(!engine.device().is_accelerated());
    }
}

#[cfg(all(
    feature = "openvino",
    not(any(
        feature = "metal",
        feature = "cuda",
        feature = "rocm",
        feature = "vulkan"
    ))
))]
mod openvino {
    use super::*;

    #[test]
    fn openvino_auto_yields_npu() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Npu,
            "OpenVINO-only build: Auto must yield Npu"
        );
    }

    #[test]
    fn openvino_npu_pref_succeeds() {
        let engine = LlamaCppEngine::new(DevicePreference::Npu).unwrap();
        assert_eq!(engine.device(), InferenceDevice::Npu);
        assert!(engine.device().is_accelerated());
    }

    #[test]
    fn openvino_cpu_override_works() {
        let engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Cpu,
            "OpenVINO build: CPU override must still work"
        );
    }

    #[test]
    fn openvino_npu_serialises_as_npu() {
        let engine = LlamaCppEngine::new(DevicePreference::Npu).unwrap();
        let json = serde_json::to_string(&engine.device()).unwrap();
        assert_eq!(
            json, "\"npu\"",
            "OpenVINO device must serialise as 'npu' in API responses"
        );
    }
}

#[cfg(all(
    feature = "ryzenai",
    not(any(
        feature = "metal",
        feature = "cuda",
        feature = "rocm",
        feature = "vulkan",
        feature = "openvino"
    ))
))]
mod ryzenai {
    use super::*;

    #[test]
    fn ryzenai_auto_yields_ryzenai() {
        let engine = LlamaCppEngine::new(DevicePreference::Auto).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::RyzenAi,
            "Ryzen AI-only build: Auto must yield RyzenAi"
        );
    }

    #[test]
    fn ryzenai_pref_succeeds() {
        let engine = LlamaCppEngine::new(DevicePreference::RyzenAi).unwrap();
        assert_eq!(engine.device(), InferenceDevice::RyzenAi);
        assert!(engine.device().is_accelerated());
    }

    #[test]
    fn ryzenai_cpu_override_works() {
        let engine = LlamaCppEngine::new(DevicePreference::Cpu).unwrap();
        assert_eq!(
            engine.device(),
            InferenceDevice::Cpu,
            "Ryzen AI build: CPU override must still work"
        );
    }

    #[test]
    fn ryzenai_device_serialises_as_ryzenai() {
        let engine = LlamaCppEngine::new(DevicePreference::RyzenAi).unwrap();
        let json = serde_json::to_string(&engine.device()).unwrap();
        assert_eq!(
            json, "\"ryzenai\"",
            "Ryzen AI device must serialise as 'ryzenai' in API responses"
        );
    }
}

#[cfg(not(feature = "ryzenai"))]
#[test]
fn ryzenai_pref_fails_when_not_compiled() {
    let result = LlamaCppEngine::new(DevicePreference::RyzenAi);
    assert!(
        result.is_err(),
        "DevicePreference::RyzenAi must fail when ryzenai is not compiled: got Ok"
    );

    let err = result.unwrap_err().to_string().to_lowercase();
    assert!(
        err.contains("ryzen") || err.contains("--features"),
        "error message should explain that the Ryzen AI backend is not available: {err}"
    );
}

#[test]
fn stub_engine_always_reports_cpu() {
    // Stub engine always reports CPU, never accelerated
    assert_eq!(StubEngine.device(), InferenceDevice::Cpu);
    assert!(
        !StubEngine.device().is_accelerated(),
        "StubEngine must not claim to be accelerated regardless of compiled features"
    );
}

#[cfg(not(any(
    feature = "metal",
    feature = "cuda",
    feature = "rocm",
    feature = "vulkan"
)))]
#[test]
fn gpu_error_message_names_rebuild_options() {
    let err = LlamaCppEngine::new(DevicePreference::Gpu)
        .unwrap_err()
        .to_string()
        .to_lowercase();

    // Error message should list cargo feature flags to enable GPU
    let mentions_option = err.contains("metal")
        || err.contains("cuda")
        || err.contains("rocm")
        || err.contains("vulkan")
        || err.contains("--features");

    assert!(
        mentions_option,
        "GPU error must name at least one rebuild option: {err}"
    );
}

#[cfg(not(feature = "openvino"))]
#[test]
fn npu_error_message_names_openvino() {
    let err = LlamaCppEngine::new(DevicePreference::Npu)
        .unwrap_err()
        .to_string()
        .to_lowercase();

    assert!(
        err.contains("openvino") || err.contains("--features"),
        "NPU error must mention OpenVINO or --features: {err}"
    );
}

#[cfg(not(feature = "ryzenai"))]
#[test]
fn ryzenai_error_message_names_rebuild_options() {
    let err = LlamaCppEngine::new(DevicePreference::RyzenAi)
        .unwrap_err()
        .to_string()
        .to_lowercase();

    assert!(
        err.contains("ryzen") || err.contains("--features"),
        "Ryzen AI error must name the rebuild option: {err}"
    );
}
