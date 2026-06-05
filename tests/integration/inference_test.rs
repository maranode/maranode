//! tests for stub inference engine and InferenceEngine trait

use std::path::Path;
use std::sync::Arc;

use maranode_common::models::{ChatMessage, ChatRole, InferenceDevice, ModelId};
use maranode_inference::{
    engine::InferenceEngine,
    stub::StubEngine,
    types::{InferenceRequest, Token},
};
use tokio::sync::mpsc;

fn make_request(model: &str) -> InferenceRequest {
    InferenceRequest {
        request_id: "test-req-1".into(),
        model: ModelId::parse(model).unwrap(),
        model_path: std::path::PathBuf::from("/tmp/fake.gguf"),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: "Hello, Maranode!".into(),
        }],
        temperature: 0.7,
        max_tokens: 64,
        stop_sequences: vec![],
        stream: false,
    }
}

#[tokio::test]
async fn stub_generate_returns_response() {
    let engine = StubEngine;
    let req = make_request("llama3.2:3b");

    let resp = engine.generate(req).await.unwrap();

    assert!(
        !resp.content.is_empty(),
        "response content must not be empty"
    );
    assert!(resp.tokens_in > 0, "tokens_in should be non-zero");
    assert!(resp.tokens_out > 0, "tokens_out should be non-zero");
    assert_eq!(resp.device, InferenceDevice::Cpu);
}

#[tokio::test]
async fn stub_generate_echoes_request_id_and_model() {
    let engine = StubEngine;
    let req = make_request("deepseek:7b");

    let resp = engine.generate(req).await.unwrap();

    assert_eq!(resp.request_id, "test-req-1");
    assert_eq!(resp.model, ModelId::parse("deepseek:7b").unwrap());
}

#[tokio::test]
async fn stub_device_is_cpu() {
    let engine = StubEngine;
    assert_eq!(engine.device(), InferenceDevice::Cpu);
}

#[tokio::test]
async fn stub_load_model_is_noop() {
    let engine = StubEngine;
    let result = engine
        .load_model("test:v1", Path::new("/tmp/model.gguf"))
        .await;
    assert!(result.is_ok(), "load_model must succeed (no-op)");
}

#[tokio::test]
async fn stub_unload_model_is_noop() {
    let engine = StubEngine;
    let result = engine.unload_model("test:v1").await;
    assert!(result.is_ok(), "unload_model must succeed (no-op)");
}

#[tokio::test]
async fn stub_generate_stream_sends_tokens() {
    let engine = StubEngine;
    let req = make_request("llama3.2:3b");

    let (tx, mut rx) = mpsc::channel::<anyhow::Result<Token>>(32);
    engine.generate_stream(req, tx).await;

    let mut received = vec![];
    while let Some(token_result) = rx.recv().await {
        let token = token_result.unwrap();
        received.push(token.clone());
        if token.is_last {
            break;
        }
    }

    assert!(
        !received.is_empty(),
        "streaming must produce at least one token"
    );
    assert!(
        received.last().unwrap().is_last,
        "last token must have is_last=true"
    );
}

#[tokio::test]
async fn engine_usable_behind_arc_dyn() {
    // Engine behind Arc<dyn InferenceEngine>
    let engine: Arc<dyn InferenceEngine> = Arc::new(StubEngine);
    let req = make_request("model:tag");
    let resp = engine.generate(req).await.unwrap();
    assert!(!resp.content.is_empty());
}
