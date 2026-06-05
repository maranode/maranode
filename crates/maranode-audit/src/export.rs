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

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
