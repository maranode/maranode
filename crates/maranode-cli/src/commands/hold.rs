use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub enum HoldCommand {
    /// generate a new hold keypair (admin runs once, gives private key to compliance)
    GenerateKey {
        #[arg(long, default_value = "your organization")]
        org_name: String,
        /// optionally seal the hold key into TPM
        #[arg(long)]
        tpm_seal: bool,
        #[arg(long, env = "MARANODE_TPM_PASSPHRASE")]
        tpm_passphrase: Option<String>,
    },

    /// place a legal hold on a range of audit entries
    Place {
        #[arg(long)]
        reason: String,
        #[arg(long)]
        seq_from: u64,
        #[arg(long)]
        seq_to: u64,
        /// ISO-8601 expiry date (optional)
        #[arg(long)]
        expires_at: Option<String>,
        /// hold private key hex (if not using TPM-sealed key)
        #[arg(long, env = "MARANODE_HOLD_KEY")]
        key_hex: Option<String>,
        #[arg(long)]
        tpm_seal: bool,
        #[arg(long, env = "MARANODE_TPM_PASSPHRASE")]
        tpm_passphrase: Option<String>,
    },

    /// sign a release payload (compliance officer does this offline with their private key)
    SignRelease {
        #[arg(long)]
        hold_id: String,
        #[arg(long)]
        released_by: String,
        #[arg(long, env = "MARANODE_HOLD_KEY")]
        privkey_hex: String,
    },

    /// release a legal hold (admin submits the compliance signature)
    Release {
        hold_id: String,
        #[arg(long)]
        release_sig_b64: String,
    },

    /// list all holds (active and released)
    List,
}

pub async fn run(cmd: HoldCommand, host: &str) -> Result<()> {
    let client = reqwest::Client::new();

    match cmd {
        HoldCommand::GenerateKey { org_name, tpm_seal, tpm_passphrase } => {
            let body = serde_json::json!({
                "tpm_seal": tpm_seal,
                "tpm_passphrase": tpm_passphrase,
                "org_name": org_name,
            });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/legal-hold/generate-key"))
                .json(&body)
                .send().await?
                .error_for_status()?
                .json().await?;
            println!("Hold keypair generated.");
            println!("  Public key (server-side): {}", resp["pubkey_b64"].as_str().unwrap_or("?"));
            println!("  Private key (STORE OFFLINE): {}", resp["privkey_hex"].as_str().unwrap_or("?"));
            if let Some(path) = resp["recovery_card_path"].as_str() {
                println!("  Recovery card saved to: {path}");
            }
            println!("  TPM sealed: {}", resp["tpm_sealed"]);
        }

        HoldCommand::Place { reason, seq_from, seq_to, expires_at, key_hex, tpm_seal, tpm_passphrase } => {
            let body = serde_json::json!({
                "reason": reason,
                "seq_from": seq_from,
                "seq_to": seq_to,
                "expires_at": expires_at,
                "key_hex": key_hex,
                "tpm_seal": tpm_seal,
                "tpm_passphrase": tpm_passphrase,
            });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/legal-hold/place"))
                .json(&body)
                .send().await?
                .error_for_status()?
                .json().await?;
            println!("Legal hold placed: {}", resp["hold_id"].as_str().unwrap_or("?"));
            println!("  Seq range: {}-{}", resp["seq_from"], resp["seq_to"]);
            println!("  Placed at: {}", resp["placed_at"].as_str().unwrap_or("?"));
        }

        HoldCommand::SignRelease { hold_id, released_by, privkey_hex } => {
            let body = serde_json::json!({
                "hold_id": hold_id,
                "released_by": released_by,
                "privkey_hex": privkey_hex,
            });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/legal-hold/sign-release"))
                .json(&body)
                .send().await?
                .error_for_status()?
                .json().await?;
            println!("Release signature generated:");
            println!("  Hold ID:    {}", resp["hold_id"].as_str().unwrap_or("?"));
            println!("  Released by:{}", resp["released_by"].as_str().unwrap_or("?"));
            println!("  Signature:  {}", resp["release_sig_b64"].as_str().unwrap_or("?"));
            println!("\nGive this signature to the admin to complete the release.");
        }

        HoldCommand::Release { hold_id, release_sig_b64 } => {
            let body = serde_json::json!({ "release_sig_b64": release_sig_b64 });
            let resp: serde_json::Value = client
                .post(format!("{host}/v1/legal-hold/release/{hold_id}"))
                .json(&body)
                .send().await?
                .error_for_status()?
                .json().await?;
            println!("Hold {} released at {}", resp["hold_id"], resp["released_at"]);
        }

        HoldCommand::List => {
            let resp: serde_json::Value = client
                .get(format!("{host}/v1/legal-hold/list"))
                .send().await?
                .error_for_status()?
                .json().await?;
            println!("Legal holds: {} total, {} active", resp["total"], resp["active"]);
            if let Some(holds) = resp["holds"].as_array() {
                for h in holds {
                    let active = h["released_at"].is_null();
                    let status = if active { "ACTIVE" } else { "released" };
                    println!(
                        "  [{}] {} seq={}-{} reason={}",
                        status,
                        h["id"].as_str().unwrap_or("?"),
                        h["seq_from"], h["seq_to"],
                        h["reason"].as_str().unwrap_or("?")
                    );
                }
            }
        }
    }

    Ok(())
}
