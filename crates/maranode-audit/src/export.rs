use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};

use maranode_common::events::AuditEntry;

use crate::log::AuditLog;

pub struct ExportFilter {
    pub workspace: Option<String>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

fn load_entries(log_path: &Path, filter: &ExportFilter) -> Result<Vec<AuditEntry>> {
    let all = AuditLog::read_recent(log_path, usize::MAX)?;
    Ok(all
        .into_iter()
        .filter(|e| {
            if let Some(ws) = &filter.workspace {
                if e.actor != *ws {
                    return false;
                }
            }
            if let Some(from) = filter.from {
                if e.ts < from {
                    return false;
                }
            }
            if let Some(to) = filter.to {
                if e.ts > to {
                    return false;
                }
            }
            true
        })
        .collect())
}

// export format for GDPR Article 30
pub fn export_gdpr(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    out.push_str("timestamp,seq,actor,activity,detail\n");
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let detail = ev
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| *k != "event")
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            e.ts.to_rfc3339(),
            e.seq,
            csv_escape(&e.actor),
            csv_escape(kind),
            csv_escape(&detail),
        ));
    }
    Ok(out)
}

// export format for HIPAA access log
pub fn export_hipaa(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    out.push_str("timestamp,seq,actor,event_type,access_detail,phi_indicator\n");
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let phi = matches!(
            kind,
            "inference_start" | "inference_complete" | "rag_retrieval" | "rag_document_ingested"
        );
        let detail = match kind {
            "inference_start" => ev["prompt_sha256"]
                .as_str()
                .map(|s| format!("prompt_sha256={}", s))
                .unwrap_or_default(),
            "inference_complete" => format!(
                "tokens_in={} tokens_out={}",
                ev["tokens_in"].as_u64().unwrap_or(0),
                ev["tokens_out"].as_u64().unwrap_or(0),
            ),
            "rag_retrieval" => ev["query_sha256"]
                .as_str()
                .map(|s| format!("query_sha256={}", s))
                .unwrap_or_default(),
            _ => String::new(),
        };
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            e.ts.to_rfc3339(),
            e.seq,
            csv_escape(&e.actor),
            csv_escape(kind),
            csv_escape(&detail),
            if phi { "yes" } else { "no" },
        ));
    }
    Ok(out)
}

// export format for SOC2 security events
pub fn export_soc2(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    out.push_str("timestamp,seq,actor,category,event_type,detail\n");
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let category = match kind {
            "daemon_start" | "daemon_stop" => "availability",
            "isolation_applied" | "config_reloaded" => "change_management",
            "model_imported" | "model_removed" => "change_management",
            "inference_start" | "inference_complete" | "inference_failed" => "logical_access",
            "rag_document_ingested" | "rag_retrieval" => "logical_access",
            "audit_verified" => "monitoring",
            _ => "other",
        };
        let detail = ev
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| *k != "event")
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            e.ts.to_rfc3339(),
            e.seq,
            csv_escape(&e.actor),
            category,
            csv_escape(kind),
            csv_escape(&detail),
        ));
    }
    Ok(out)
}

// export format for ISO 27001 security events
pub fn export_iso27001(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    out.push_str("timestamp,seq,actor,control,event_type,outcome,detail\n");
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let (control, outcome) = match kind {
            "daemon_start" | "daemon_stop" => ("A.12.1", "operational"),
            "isolation_applied" => ("A.13.1", "network_control"),
            "model_imported" => ("A.12.5", "change"),
            "model_removed" => ("A.12.5", "change"),
            "inference_start" | "inference_complete" => ("A.9.4", "access"),
            "inference_failed" => ("A.9.4", "failure"),
            "rag_document_ingested" | "rag_retrieval" => ("A.9.4", "access"),
            "config_reloaded" => ("A.12.1", "change"),
            "audit_verified" => ("A.12.4", "monitoring"),
            _ => ("A.12.4", "other"),
        };
        let detail = ev
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| *k != "event")
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            e.ts.to_rfc3339(),
            e.seq,
            csv_escape(&e.actor),
            control,
            csv_escape(kind),
            outcome,
            csv_escape(&detail),
        ));
    }
    Ok(out)
}

// CEF (Common Event Format) export — compatible with Splunk, ArcSight, Sentinel
pub fn export_cef(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let (severity, name, sig_id) = cef_meta(kind);
        let ext = cef_extension(&ev, &e.actor, e.seq);
        out.push_str(&format!(
            "CEF:0|Maranode|maranode-daemon|1.0|{}|{}|{}|rt={} {}\\n",
            sig_id,
            name,
            severity,
            e.ts.timestamp_millis(),
            ext,
        ));
    }
    Ok(out)
}

fn cef_meta(kind: &str) -> (u8, &'static str, &'static str) {
    match kind {
        "daemon_start"            => (2, "Daemon Started",               "MRN-001"),
        "daemon_stop"             => (2, "Daemon Stopped",               "MRN-002"),
        "isolation_applied"       => (5, "Isolation Mode Applied",       "MRN-010"),
        "isolation_probe"         => (5, "Isolation Probe Result",       "MRN-011"),
        "model_imported"          => (3, "Model Imported",               "MRN-020"),
        "model_removed"           => (3, "Model Removed",                "MRN-021"),
        "inference_start"         => (2, "Inference Started",            "MRN-030"),
        "inference_complete"      => (2, "Inference Completed",          "MRN-031"),
        "inference_failed"        => (7, "Inference Failed",             "MRN-032"),
        "inference_receipt"       => (3, "Inference Receipt Issued",     "MRN-033"),
        "rag_document_ingested"   => (3, "RAG Document Ingested",        "MRN-040"),
        "rag_retrieval"           => (2, "RAG Retrieval Performed",      "MRN-041"),
        "workspace_shredded"      => (8, "Workspace Crypto-Shredded",   "MRN-050"),
        "config_reloaded"         => (3, "Config Reloaded",              "MRN-060"),
        "audit_verified"          => (2, "Audit Chain Verified",         "MRN-070"),
        "binary_attested"         => (4, "Binary Attested",              "MRN-080"),
        _                         => (2, "Audit Event",                  "MRN-000"),
    }
}

fn cef_extension(ev: &serde_json::Value, actor: &str, seq: u64) -> String {
    let kind = ev["event"].as_str().unwrap_or("unknown");
    let mut parts: Vec<String> = vec![
        format!("act={}", cef_val(kind)),
        format!("src={}", cef_val(actor)),
        format!("cn1={}", seq),
        "cn1Label=auditSeq".into(),
    ];

    match kind {
        "inference_start" | "inference_complete" | "inference_failed" => {
            if let Some(id) = ev["request_id"].as_str() {
                parts.push(format!("externalId={}", cef_val(id)));
            }
            if let Some(m) = ev["model"]["name"].as_str() {
                parts.push(format!("cs1={}", cef_val(m)));
                parts.push("cs1Label=modelName".into());
            }
            if let Some(h) = ev["prompt_sha256"].as_str() {
                parts.push(format!("cs2={}", cef_val(h)));
                parts.push("cs2Label=promptSha256".into());
            }
        }
        "isolation_probe" => {
            let isolated = ev["isolated"].as_bool().unwrap_or(true);
            parts.push(format!("outcome={}", if isolated { "success" } else { "failure" }));
            parts.push(format!("cs3={}", cef_val(ev["iptables_hash"].as_str().unwrap_or(""))));
            parts.push("cs3Label=iptablesHash".into());
        }
        "workspace_shredded" => {
            if let Some(slug) = ev["slug"].as_str() {
                parts.push(format!("fname={}", cef_val(slug)));
            }
        }
        "model_imported" | "model_removed" => {
            if let Some(sha) = ev["sha256"].as_str() {
                parts.push(format!("fileHash={}", cef_val(sha)));
            }
        }
        _ => {}
    }

    parts.join(" ")
}

fn cef_val(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace('\n', "\\n")
}

// LEEF 2.0 (Log Event Extended Format) export — IBM QRadar
pub fn export_leef(log_path: &Path, filter: &ExportFilter) -> Result<String> {
    let entries = load_entries(log_path, filter)?;
    let mut out = String::new();
    for e in &entries {
        let ev = serde_json::to_value(&e.event)?;
        let kind = ev["event"].as_str().unwrap_or("unknown");
        let event_id = leef_event_id(kind);
        let attrs = leef_attributes(&ev, &e.actor, e.seq, &e.ts);
        out.push_str(&format!(
            "LEEF:2.0|Maranode|maranode-daemon|1.0|{}|\t{}\n",
            event_id, attrs,
        ));
    }
    Ok(out)
}

fn leef_event_id(kind: &str) -> &'static str {
    match kind {
        "daemon_start"          => "DaemonStarted",
        "daemon_stop"           => "DaemonStopped",
        "isolation_applied"     => "IsolationApplied",
        "isolation_probe"       => "IsolationProbe",
        "model_imported"        => "ModelImported",
        "model_removed"         => "ModelRemoved",
        "inference_start"       => "InferenceStarted",
        "inference_complete"    => "InferenceCompleted",
        "inference_failed"      => "InferenceFailed",
        "inference_receipt"     => "InferenceReceipt",
        "rag_document_ingested" => "RagDocIngested",
        "rag_retrieval"         => "RagRetrieval",
        "workspace_shredded"    => "WorkspaceShredded",
        "config_reloaded"       => "ConfigReloaded",
        "audit_verified"        => "AuditVerified",
        "binary_attested"       => "BinaryAttested",
        _                       => "AuditEvent",
    }
}

fn leef_attributes(ev: &serde_json::Value, actor: &str, seq: u64, ts: &DateTime<Utc>) -> String {
    let kind = ev["event"].as_str().unwrap_or("");
    let mut parts: Vec<String> = vec![
        format!("devTime={}", ts.to_rfc3339()),
        format!("usrName={}", leef_val(actor)),
        format!("src=maranode"),
        format!("auditSeq={}", seq),
        format!("eventType={}", leef_val(kind)),
    ];

    match kind {
        "inference_start" | "inference_complete" | "inference_failed" => {
            if let Some(id) = ev["request_id"].as_str() {
                parts.push(format!("requestId={}", leef_val(id)));
            }
            if let Some(sha) = ev["prompt_sha256"].as_str() {
                parts.push(format!("promptSha256={}", leef_val(sha)));
            }
            if let Some(ti) = ev["tokens_in"].as_u64() {
                parts.push(format!("tokensIn={}", ti));
            }
            if let Some(to) = ev["tokens_out"].as_u64() {
                parts.push(format!("tokensOut={}", to));
            }
        }
        "isolation_probe" => {
            let isolated = ev["isolated"].as_bool().unwrap_or(true);
            parts.push(format!("isolated={}", isolated));
            parts.push(format!("outcome={}", if isolated { "success" } else { "failure" }));
        }
        "workspace_shredded" => {
            if let Some(slug) = ev["slug"].as_str() {
                parts.push(format!("resourceId={}", leef_val(slug)));
            }
        }
        _ => {}
    }

    parts.join("\t")
}

fn leef_val(s: &str) -> String {
    s.replace('\t', " ").replace('\n', " ")
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
