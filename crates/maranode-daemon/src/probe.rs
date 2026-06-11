use std::sync::atomic::Ordering;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::time::interval;
use tracing::{info, warn};

use maranode_api::AppState;
use maranode_common::events::{AuditEvent, ProbeResult};

// hosts we try to reach; if any succeed the air-gap is broken
const PROBE_TARGETS: &[(&str, u16)] = &[
    ("1.1.1.1", 53),
    ("8.8.8.8", 53),
    ("93.184.216.34", 80), // example.com
];

const PROBE_INTERVAL_SECS: u64 = 300; // 5 min
const CONNECT_TIMEOUT_MS: u64 = 2000;

pub fn spawn(state: AppState) {
    tokio::spawn(run_probe_loop(state));
}

async fn run_probe_loop(state: AppState) {
    let mut ticker = interval(Duration::from_secs(PROBE_INTERVAL_SECS));
    ticker.tick().await; // skip the first immediate tick so daemon starts fast

    loop {
        ticker.tick().await;

        // only probe when isolation is supposed to be active
        let rt = state.rt();
        if !rt.air_gap {
            continue;
        }

        let (isolated, results, iptables_hash) = run_probe().await;

        state.isolation_ok.store(isolated, Ordering::Relaxed);

        if isolated {
            info!("isolation probe: all egress blocked — air-gap intact");
        } else {
            warn!("isolation probe: egress DETECTED — air-gap may be broken");
        }

        let _ = state
            .audit
            .append(
                "probe",
                AuditEvent::IsolationProbe {
                    isolated,
                    probe_results: results,
                    iptables_hash,
                },
            )
            .await;
    }
}

async fn run_probe() -> (bool, Vec<ProbeResult>, String) {
    let mut results = Vec::new();
    let mut any_reachable = false;

    for &(host, port) in PROBE_TARGETS {
        let reachable = try_connect(host, port).await;
        if reachable {
            any_reachable = true;
        }
        results.push(ProbeResult {
            host: host.to_string(),
            port,
            reachable,
        });
    }

    let iptables_hash = snapshot_iptables();
    let isolated = !any_reachable;
    (isolated, results, iptables_hash)
}

async fn try_connect(host: &str, port: u16) -> bool {
    let addr = format!("{}:{}", host, port);
    let timeout = Duration::from_millis(CONNECT_TIMEOUT_MS);
    match tokio::time::timeout(timeout, tokio::net::TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}

fn snapshot_iptables() -> String {
    #[cfg(target_os = "linux")]
    {
        match std::process::Command::new("iptables-save").output() {
            Ok(out) if out.status.success() => {
                let hash = Sha256::digest(&out.stdout);
                return format!("{:x}", hash);
            }
            _ => {}
        }
    }
    String::new()
}
