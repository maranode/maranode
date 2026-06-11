use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;

use maranode_audit::AuditLog;
use maranode_audit::log::{default_key_path, default_log_path};
use maranode_common::events::AuditEvent;
use maranode_store::{kek, WorkspaceDb};

#[derive(Subcommand)]
pub enum WorkspaceCommand {
    /// destroy the workspace DEK making all encrypted data permanently unreadable
    Shred {
        /// workspace slug (e.g. "default")
        id: String,
        /// skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(cmd: WorkspaceCommand, data_dir: &Path) -> Result<()> {
    match cmd {
        WorkspaceCommand::Shred { id, yes } => shred(data_dir, &id, yes),
    }
}

fn shred(data_dir: &Path, slug: &str, yes: bool) -> Result<()> {
    if !yes {
        eprintln!(
            "warning: shredding workspace '{}' destroys its encryption key.\n\
             all encrypted chunks and summaries will be permanently unreadable.\n\
             this cannot be undone.\n\
             pass --yes to confirm.",
            slug
        );
        return Ok(());
    }

    let master_key = kek::load_or_create(&kek::default_kek_path(data_dir))
        .context("loading master KEK")?;

    let ws_db_path = data_dir.join("workspaces.db");
    let db = WorkspaceDb::open_with_kek(&ws_db_path, master_key)
        .context("opening workspace database")?;

    let found = db.destroy_dek(slug)
        .with_context(|| format!("destroying DEK for workspace '{}'", slug))?;

    if !found {
        anyhow::bail!("workspace '{}' not found", slug);
    }

    let statement = format!(
        "the encryption key (DEK) for workspace '{}' has been permanently destroyed. \
         all data encrypted under this key is now cryptographically unreadable. \
         this action cannot be reversed.",
        slug
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let audit = AuditLog::open(&default_log_path(data_dir), &default_key_path(data_dir))?;
        audit
            .append(
                "cli",
                AuditEvent::WorkspaceShredded {
                    slug: slug.to_string(),
                    actor: "cli".to_string(),
                    statement: statement.clone(),
                },
            )
            .await
    })?;

    println!("{}", statement);
    println!("deletion certificate written to audit log.");

    Ok(())
}
