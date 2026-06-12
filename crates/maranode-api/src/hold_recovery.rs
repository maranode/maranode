use std::path::Path;

use anyhow::Result;
use chrono::Utc;

use crate::legal_hold::holds_dir;

// produce a human-readable recovery card for the compliance officer.
// this is intentionally verbose and non-technical.
pub fn generate_recovery_card(
    hold_key_hex: &str,
    pubkey_b64: &str,
    org_name: &str,
) -> String {
    let now = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    format!(
        r#"==============================================================
   MARANODE LEGAL HOLD KEY — RECOVERY CARD
==============================================================
Organization: {org_name}
Generated:    {now}

IMPORTANT: Store this document securely. This key allows
releasing legal holds on audit data. Treat like a physical
key to a safe deposit box.

PUBLIC KEY (server-side, for verification only):
{pubkey_b64}

PRIVATE KEY (keep offline — never share with IT):
{hold_key_hex}

HOW TO RELEASE A HOLD:
1. Contact your Maranode administrator for the hold ID and
   release payload details.
2. Use: maranode hold sign-release --hold-id <ID> \
       --released-by <your-name> --key-hex <PRIVATE KEY ABOVE>
3. Give the resulting signature to the administrator who
   will complete the release via the API.

The administrator CANNOT release a hold without this key.
IT access alone is not sufficient.
==============================================================
"#
    )
}

pub fn export_hold_backup(data_dir: &Path, hold_key_hex: &str, pubkey_b64: &str) -> Result<()> {
    let backup_dir = holds_dir(data_dir);
    std::fs::create_dir_all(&backup_dir)?;
    let path = backup_dir.join("HOLD-KEY-RECOVERY.txt");
    let card = generate_recovery_card(hold_key_hex, pubkey_b64, "your organization");
    std::fs::write(&path, card)?;
    Ok(())
}
