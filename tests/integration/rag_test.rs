//! tests for rag document ingest and retrieval
//!
//! run with: cargo test --test rag

use std::sync::Arc;

use maranode_rag::{
    config::RagConfig,
    embed::Embedder,
    engine::{RagEngine, RetrievedChunk},
};
use anyhow::Result;

struct FakeEmbedder;

const VOCAB: &[&str] = &[
    "patient",
    "blood",
    "pressure",
    "contract",
    "liability",
    "budget",
    "revenue",
    "expense",
    "rust",
    "compiler",
];

#[async_trait::async_trait]
impl Embedder for FakeEmbedder {
    fn model_label(&self) -> String {
        "fake-embedder-v1".into()
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|t| {
                let lower = t.to_lowercase();
                let mut v: Vec<f32> = VOCAB
                    .iter()
                    .map(|w| lower.matches(w).count() as f32)
                    .collect();
                v.push(1.0);
                v
            })
            .collect())
    }
}

fn engine_with(min_score: f32, top_k: usize) -> RagEngine {
    let cfg = RagConfig {
        enabled: true,
        chunk_size: 300,
        chunk_overlap: 30,
        top_k,
        min_score,
        max_context_chars: 2000,
        ..Default::default()
    };
    RagEngine::in_memory(Arc::new(FakeEmbedder), cfg).unwrap()
}

fn engine() -> RagEngine {
    engine_with(0.0, 5)
}

#[tokio::test]
async fn ingest_shows_in_collection_list() {
    let e = engine();
    let stats = e
        .ingest(
            "medical",
            "report.txt",
            "The patient blood pressure was elevated. Blood tests showed normal values.",
        )
        .await
        .unwrap();

    assert_eq!(stats.collection, "medical");
    assert!(stats.chunks >= 1);

    let collections = e.list_collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "medical");
    assert_eq!(collections[0].chunks, stats.chunks);
}

#[tokio::test]
async fn retrieve_from_empty_collection_returns_empty() {
    let e = engine();
    let hits = e.retrieve("ghost", "anything", Some(5)).await;
    match hits {
        Ok(v) => assert!(v.is_empty()),
        Err(_) => { /* missing collection is also ok */ }
    }
}

#[tokio::test]
async fn multi_collection_isolation() {
    let e = engine();

    e.ingest(
        "medical",
        "medical.txt",
        "Patient blood pressure readings were taken three times.",
    )
    .await
    .unwrap();

    e.ingest(
        "legal",
        "contract.txt",
        "The contract liability clause covers all third-party losses.",
    )
    .await
    .unwrap();

    let hits = e
        .retrieve("legal", "patient blood pressure", Some(3))
        .await
        .unwrap();
    for h in &hits {
        assert!(
            !h.text.to_lowercase().contains("blood"),
            "medical content leaked into legal collection: {:?}",
            h.text
        );
    }

    let hits = e
        .retrieve("medical", "contract liability", Some(3))
        .await
        .unwrap();
    for h in &hits {
        assert!(
            !h.text.to_lowercase().contains("contract"),
            "legal content leaked into medical collection: {:?}",
            h.text
        );
    }
}

#[tokio::test]
async fn min_score_filters_irrelevant_chunks() {
    let e = engine_with(0.99, 5);

    e.ingest(
        "default",
        "mixed.txt",
        "The patient blood pressure was elevated. \
         The rust compiler produces fast binaries.",
    )
    .await
    .unwrap();

    let hits = e
        .retrieve("default", "blood pressure patient", Some(5))
        .await
        .unwrap();

    for h in &hits {
        assert!(
            h.score >= 0.99,
            "chunk below min_score should have been filtered: score={} text={:?}",
            h.score,
            h.text
        );
    }
}

#[tokio::test]
async fn delete_collection_removes_all_data() {
    let e = engine();

    e.ingest(
        "temp",
        "doc.txt",
        "Patient blood pressure data from clinical trial.",
    )
    .await
    .unwrap();

    let hits = e.retrieve("temp", "blood pressure", Some(3)).await.unwrap();
    assert!(!hits.is_empty(), "should find chunks before deletion");

    let deleted = e.delete_collection("temp").unwrap();
    assert!(
        deleted,
        "delete_collection should return true when collection existed"
    );

    let cols = e.list_collections().unwrap();
    assert!(!cols.iter().any(|c| c.name == "temp"));

    let hits_after = e.retrieve("temp", "blood pressure", Some(3)).await;
    match hits_after {
        Ok(v) => assert!(v.is_empty(), "deleted collection should return no hits"),
        Err(_) => { /* error after delete is also ok */ }
    }
}

#[tokio::test]
async fn delete_nonexistent_collection_returns_false() {
    let e = engine();
    let deleted = e.delete_collection("does-not-exist").unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn top_k_caps_results() {
    let e = engine_with(0.0, 2); // RAG config: top_k is 2

    let long_doc = "Patient blood pressure. ".repeat(50) + &"Rust compiler speed. ".repeat(50);
    e.ingest("default", "long.txt", &long_doc).await.unwrap();

    let hits = e
        .retrieve("default", "blood pressure patient", Some(2))
        .await
        .unwrap();
    assert!(
        hits.len() <= 2,
        "top_k=2 must return at most 2 chunks, got {}",
        hits.len()
    );
}

#[tokio::test]
async fn context_prompt_respects_max_chars() {
    let cfg = RagConfig {
        enabled: true,
        max_context_chars: 100,
        ..Default::default()
    };
    let e = RagEngine::in_memory(Arc::new(FakeEmbedder), cfg).unwrap();

    let chunks: Vec<RetrievedChunk> = (0..20)
        .map(|i| RetrievedChunk {
            source: format!("doc{}.txt", i),
            ordinal: i,
            text: "x".repeat(50),
            score: 0.9,
            page_number: 0,
            section: None,
        })
        .collect();

    let prompt = e.build_context_prompt(&chunks).unwrap();
    let context_section = prompt.split("CONTEXT:").nth(1).unwrap_or("").to_string();
    assert!(
        context_section.len() <= 200,
        "context section too large ({} chars); max_context_chars cap not respected",
        context_section.len()
    );
}

#[tokio::test]
async fn context_prompt_citation_format() {
    let e = engine();
    let chunks = vec![
        RetrievedChunk {
            source: "alpha.txt".into(),
            ordinal: 0,
            text: "First relevant fact.".into(),
            score: 0.95,
            page_number: 0,
            section: None,
        },
        RetrievedChunk {
            source: "beta.txt".into(),
            ordinal: 1,
            text: "Second relevant fact.".into(),
            score: 0.90,
            page_number: 0,
            section: None,
        },
    ];

    let prompt = e.build_context_prompt(&chunks).unwrap();
    assert!(prompt.contains("[1]"), "prompt must contain [1]");
    assert!(prompt.contains("[2]"), "prompt must contain [2]");
    assert!(prompt.contains("alpha.txt"));
    assert!(prompt.contains("beta.txt"));
    assert!(
        prompt.contains("ONLY"),
        "grounding instruction must say ONLY"
    );
}

#[tokio::test]
async fn second_ingest_to_same_collection_adds_chunks() {
    let e = engine();

    e.ingest("shared", "doc1.txt", "Patient blood pressure elevated.")
        .await
        .unwrap();
    let before = e.list_collections().unwrap();
    let chunks_before = before.iter().find(|c| c.name == "shared").unwrap().chunks;

    e.ingest("shared", "doc2.txt", "Contract liability clause is broad.")
        .await
        .unwrap();
    let after = e.list_collections().unwrap();
    let chunks_after = after.iter().find(|c| c.name == "shared").unwrap().chunks;

    assert!(
        chunks_after > chunks_before,
        "second ingest should increase chunk count: {} -> {}",
        chunks_before,
        chunks_after
    );
}

#[tokio::test]
async fn empty_document_is_rejected() {
    let e = engine();
    let result = e.ingest("default", "empty.txt", "").await;
    assert!(result.is_err(), "empty document must return an error");
}

#[tokio::test]
async fn whitespace_only_document_is_rejected() {
    let e = engine();
    let result = e.ingest("default", "blank.txt", "   \n\t\n  ").await;
    assert!(
        result.is_err(),
        "whitespace-only document must return an error"
    );
}
