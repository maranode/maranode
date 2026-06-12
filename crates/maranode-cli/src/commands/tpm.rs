use anyhow::Result;
use clap::Subcommand;

use maranode_attestation::{
    export_recovery_bundle, import_recovery_bundle, is_sealed, is_tpm2_tools_available,
    read_rotation_log, rotate_in_place, seal, seal_status, unseal, PcrPolicy,
};

#[derive(Subcommand)]
pub enum TpmCommand {
    /// show TPM availability, sealed key status, and current PCR values
    Status,

    /// capture current PCR values and write a PCR policy file for the given profile
    CapturePcrs {
        /// profile: "server" (PCR 0,7) or "workstation" (PCR 0,7,11)
        #[arg(long, default_value = "server")]
        profile: String,
        /// custom PCR indices (comma-separated), overrides --profile
        #[arg(long)]
        pcrs: Option<String>,
    },

    /// seal a key purpose using the current PCR policy
    Seal {
        /// key purpose: workspace-kek, audit-hmac, or admin-cred
        #[arg(long)]
        purpose: String,
        /// PCR indices to seal against (comma-separated, e.g. 0,7)
        #[arg(long, default_value = "0,7")]
        pcrs: String,
        /// passphrase for software fallback (required when TPM not available)
        #[arg(long, env = "MARANODE_TPM_PASSPHRASE")]
        passphrase: Option<String>,
    },

    /// test unseal for a given purpose (does not return key material — just confirms it works)
    UnsealTest {
        #[arg(long)]
        purpose: String,
        #[arg(long, env = "MARANODE_TPM_PASSPHRASE")]
        passphrase: Option<String>,
    },

    /// verify current PCR values match the saved PCR policy
    VerifyPcrs,

    /// export an encrypted recovery bundle for all sealed key purposes
    ExportRecovery {
        /// output file path for the bundle
        #[arg(long)]
        out: std::path::PathBuf,
        /// passphrase to encrypt the recovery bundle
        #[arg(long, env = "MARANODE_RECOVERY_PASSPHRASE")]
        passphrase: String,
        /// key purposes to include (comma-separated)
        #[arg(long, default_value = "workspace-kek,audit-hmac")]
        purposes: String,
    },

    /// import a recovery bundle and re-seal keys (for TPM replacement or firmware update)
    ImportRecovery {
        /// path to the recovery bundle file
        #[arg(long)]
        bundle: std::path::PathBuf,
        /// passphrase that was used when exporting the bundle
        #[arg(long, env = "MARANODE_RECOVERY_PASSPHRASE")]
        passphrase: String,
        /// PCR list for re-sealing (e.g. sha256:0,7); uses TPM default if omitted
        #[arg(long)]
        pcrs: Option<String>,
    },

    /// rotate a sealed key in-place (re-seal with new PCRs or passphrase)
    Rotate {
        /// key purpose to rotate
        #[arg(long)]
        purpose: String,
        /// current passphrase / software key
        #[arg(long, env = "MARANODE_TPM_PASSPHRASE")]
        passphrase: Option<String>,
        /// new passphrase after rotation (defaults to same as current)
        #[arg(long)]
        new_passphrase: Option<String>,
        /// new PCR list (e.g. sha256:0,7), leaves unchanged if omitted
        #[arg(long)]
        pcrs: Option<String>,
        /// reason recorded in rotation log
        #[arg(long, default_value = "manual rotation")]
        reason: String,
    },

    /// show rotation history from the rotation log
    RotationLog,
}

pub async fn run(cmd: TpmCommand, data_dir: &std::path::Path) -> Result<()> {
    match cmd {
        TpmCommand::Status => {
            let tpm_avail = is_tpm2_tools_available();
            println!("tpm2-tools available: {}", tpm_avail);

            let purposes = ["workspace-kek", "audit-hmac", "admin-cred"];
            for purpose in purposes {
                if is_sealed(purpose, data_dir) {
                    if let Some(meta) = seal_status(purpose, data_dir) {
                        println!(
                            "  {} sealed ({:?}, backend={:?}, pcrs={})",
                            purpose,
                            meta.sealed_at,
                            meta.backend,
                            meta.pcr_list.as_deref().unwrap_or("none")
                        );
                    }
                } else {
                    println!("  {} not sealed", purpose);
                }
            }

            let policy_path = PcrPolicy::policy_path(data_dir);
            if policy_path.exists() {
                match PcrPolicy::load(data_dir) {
                    Ok(pol) => {
                        let ok = pol.is_satisfied();
                        println!("pcr-policy: {} PCRs — satisfied={}", pol.pcrs.len(), ok);
                    }
                    Err(e) => println!("pcr-policy: error reading: {e}"),
                }
            } else {
                println!("pcr-policy: not configured");
            }
        }

        TpmCommand::CapturePcrs { profile, pcrs } => {
            let policy = if let Some(custom) = pcrs {
                let indices: Vec<u8> = custom
                    .split(',')
                    .map(|s| s.trim().parse::<u8>())
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(|_| anyhow::anyhow!("invalid PCR index in '{}'", custom))?;
                let mut pol = PcrPolicy::from_current(&indices)?;
                pol.description = format!("custom PCRs: {}", custom);
                pol.save(data_dir)?;
                pol
            } else {
                match profile.as_str() {
                    "server" => PcrPolicy::server_profile(data_dir)?,
                    "workstation" => PcrPolicy::workstation_profile(data_dir)?,
                    other => anyhow::bail!("unknown profile '{}' (use server or workstation)", other),
                }
            };

            println!("PCR policy saved ({} PCRs):", policy.pcrs.len());
            for (idx, val) in &policy.pcrs {
                println!("  PCR{}: {}", idx, val);
            }
        }

        TpmCommand::Seal { purpose, pcrs, passphrase } => {
            let pcr_list = format!("sha256:{}", pcrs);
            let passphrase = passphrase.as_deref().unwrap_or("");

            // read the key bytes from the plain file depending on purpose
            let key_bytes = load_key_bytes_for_purpose(&purpose, data_dir)?;

            let meta = seal(&key_bytes, &purpose, data_dir, Some(&pcr_list), passphrase)?;
            println!(
                "Sealed {} (backend={:?}, pcrs={})",
                purpose,
                meta.backend,
                meta.pcr_list.as_deref().unwrap_or("none")
            );
        }

        TpmCommand::UnsealTest { purpose, passphrase } => {
            let passphrase = passphrase.as_deref().unwrap_or("");
            match unseal(&purpose, data_dir, passphrase) {
                Ok(bytes) => println!("Unseal OK ({} bytes returned)", bytes.len()),
                Err(e) => {
                    eprintln!("Unseal failed: {e}");
                    std::process::exit(1);
                }
            }
        }

        TpmCommand::VerifyPcrs => {
            let policy = PcrPolicy::load(data_dir)
                .map_err(|_| anyhow::anyhow!("no PCR policy found — run `maranode tpm capture-pcrs` first"))?;
            match policy.verify_current() {
                Ok(()) => println!("PCR policy satisfied — system state matches expectations"),
                Err(e) => {
                    eprintln!("PCR policy VIOLATION:\n{e}");
                    std::process::exit(1);
                }
            }
        }

        TpmCommand::ExportRecovery { out, passphrase, purposes } => {
            let purpose_list: Vec<&str> = purposes.split(',').map(|s| s.trim()).collect();
            let bundle = export_recovery_bundle(&purpose_list, data_dir, &passphrase)?;
            std::fs::write(&out, &bundle)?;
            println!("Recovery bundle written to {} ({} bytes, {} purposes)", out.display(), bundle.len(), purpose_list.len());
        }

        TpmCommand::ImportRecovery { bundle, passphrase, pcrs } => {
            let bundle_bytes = std::fs::read(&bundle)?;
            let records = import_recovery_bundle(&bundle_bytes, data_dir, &passphrase, pcrs.as_deref())?;
            println!("Recovery import complete ({} keys re-sealed):", records.len());
            for r in &records {
                println!(
                    "  {} — old={:?} new={:?} pcrs={}",
                    r.purpose,
                    r.old_backend,
                    r.new_backend,
                    r.new_pcr_list.as_deref().unwrap_or("none")
                );
            }
        }

        TpmCommand::Rotate { purpose, passphrase, new_passphrase, pcrs, reason } => {
            let old_pass = passphrase.as_deref().unwrap_or("");
            let new_pass = new_passphrase.as_deref().unwrap_or(old_pass);
            let record = rotate_in_place(
                &purpose,
                data_dir,
                old_pass,
                pcrs.as_deref(),
                new_pass,
                &reason,
            )?;
            println!(
                "Rotated {} (reason: {}, old={:?} new={:?}, pcrs={})",
                record.purpose,
                record.reason,
                record.old_backend,
                record.new_backend,
                record.new_pcr_list.as_deref().unwrap_or("none")
            );
        }

        TpmCommand::RotationLog => {
            let records = read_rotation_log(data_dir)?;
            if records.is_empty() {
                println!("No rotation records found.");
            } else {
                for r in records {
                    println!(
                        "{} {} — {} ({:?} → {:?})",
                        r.rotated_at.format("%Y-%m-%d %H:%M:%S"),
                        r.purpose,
                        r.reason,
                        r.old_backend,
                        r.new_backend
                    );
                }
            }
        }
    }

    Ok(())
}

fn load_key_bytes_for_purpose(purpose: &str, data_dir: &std::path::Path) -> Result<Vec<u8>> {
    match purpose {
        "workspace-kek" => {
            let path = maranode_store::kek::default_kek_path(data_dir);
            let hex = std::fs::read_to_string(&path)
                .map_err(|_| anyhow::anyhow!("workspace-kek not found at {}", path.display()))?;
            hex::decode(hex.trim()).map_err(|e| anyhow::anyhow!("invalid kek hex: {e}"))
        }
        "audit-hmac" => {
            let path = maranode_audit::log::default_key_path(data_dir);
            std::fs::read(&path)
                .map_err(|_| anyhow::anyhow!("audit-hmac key not found at {}", path.display()))
        }
        other => anyhow::bail!(
            "unknown purpose '{}' (use workspace-kek, audit-hmac, or admin-cred)",
            other
        ),
    }
}
