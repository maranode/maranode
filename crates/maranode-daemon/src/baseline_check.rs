use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use maranode_audit::AuditLog;
use maranode_common::baseline::{output_sha256, Baseline};
use maranode_common::events::AuditEvent;
use maranode_common::models::{ChatMessage, ChatRole, ModelId};
use maranode_inference::{InferenceEngine, InferenceRequest};

use crate::config::DriftAction;

pub struct BaselineChecker {
    pub baselines_dir: PathBuf,
    pub drift_action: DriftAction,
}

impl BaselineChecker {
    pub fn new(data_dir: &Path, baselines_dir: Option<PathBuf>, drift_action: DriftAction) -> Self {
        let dir = baselines_dir.unwrap_or_else(|| data_dir.join("baselines"));
        Self {
            baselines_dir: dir,
            drift_action,
        }
    }

    pub fn baseline_path(&self, model_sha256: &str) -> PathBuf {
        self.baselines_dir.join(format!("{}.mrn-baseline", model_sha256))
    }

    pub async fn check(
        &self,
        model_id: &ModelId,
        model_sha256: &str,
        model_path: &Path,
        engine: &Arc<dyn InferenceEngine>,
        audit: &AuditLog,
    ) -> Result<bool> {
        let path = self.baseline_path(model_sha256);
        if !path.exists() {
            info!(
                "no baseline found for {} ({}), skipping integrity check",
                model_id, &model_sha256[..12]
            );
            return Ok(true);
        }

        let baseline = Baseline::load(&path)?;

        if let Err(e) = baseline.verify() {
            warn!("baseline signature invalid for {}: {}", model_id, e);
            let _ = audit
                .append(
                    "baseline",
                    AuditEvent::ModelDriftDetected {
                        model_id: model_id.to_string(),
                        model_sha256: model_sha256.to_string(),
                        vectors_failed: baseline.vectors.len(),
                        action_taken: "invalid_signature".into(),
                    },
                )
                .await;
            return Ok(false);
        }

        let mut passed = 0usize;
        let mut failed = 0usize;

        for (i, vec) in baseline.vectors.iter().enumerate() {
            let req = InferenceRequest {
                request_id: format!("baseline-{}-{}", model_id, i),
                model: model_id.clone(),
                model_path: model_path.to_path_buf(),
                messages: vec![ChatMessage {
                    role: ChatRole::User,
                    content: vec.prompt.clone(),
                }],
                temperature: vec.temperature,
                max_tokens: vec.max_tokens,
                stop_sequences: vec![],
                stream: false,
                seed: Some(vec.seed),
                deterministic: true,
            };

            match engine.generate(req).await {
                Ok(resp) => {
                    let got = output_sha256(&resp.content);
                    if got == vec.expected_sha256 {
                        passed += 1;
                    } else {
                        warn!(
                            "baseline vector {} failed for {}: expected={} got={}",
                            i, model_id, &vec.expected_sha256[..8], &got[..8]
                        );
                        failed += 1;
                    }
                }
                Err(e) => {
                    warn!("baseline vector {} error for {}: {}", i, model_id, e);
                    failed += 1;
                }
            }
        }

        let ok = failed <= baseline.max_mismatches;

        let _ = audit
            .append(
                "baseline",
                AuditEvent::ModelBaselineChecked {
                    model_id: model_id.to_string(),
                    model_sha256: model_sha256.to_string(),
                    vectors_run: baseline.vectors.len(),
                    vectors_passed: passed,
                    vectors_failed: failed,
                    baseline_signer: baseline.signer_pubkey.clone(),
                },
            )
            .await;

        if !ok {
            let action_str = match self.drift_action {
                DriftAction::Allow => "allow",
                DriftAction::Warn => "warn",
                DriftAction::Refuse => "refuse",
            };

            let _ = audit
                .append(
                    "baseline",
                    AuditEvent::ModelDriftDetected {
                        model_id: model_id.to_string(),
                        model_sha256: model_sha256.to_string(),
                        vectors_failed: failed,
                        action_taken: action_str.into(),
                    },
                )
                .await;

            match self.drift_action {
                DriftAction::Warn => {
                    warn!(
                        "BEHAVIORAL DRIFT detected for {}: {}/{} vectors failed — continuing (drift_action=warn)",
                        model_id, failed, baseline.vectors.len()
                    );
                }
                DriftAction::Refuse => {
                    anyhow::bail!(
                        "BEHAVIORAL DRIFT detected for {}: {}/{} vectors failed — refusing to load (drift_action=refuse)",
                        model_id, failed, baseline.vectors.len()
                    );
                }
                DriftAction::Allow => {}
            }
        } else {
            info!(
                "baseline check passed for {}: {}/{} vectors OK",
                model_id, passed, baseline.vectors.len()
            );
        }

        Ok(ok)
    }
}
