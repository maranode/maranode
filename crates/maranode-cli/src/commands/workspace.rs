use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;

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

    let kek_path = kek::default_kek_path(data_dir);
    let master_key = kek::load_or_create(&kek_path)
        .context("loading master KEK")?;

    let ws_db_path = data_dir.join("workspaces.db");
    let db = WorkspaceDb::open_with_kek(&ws_db_path, master_key)
        .context("opening workspace database")?;

    let found = db.destroy_dek(slug)
        .with_context(|| format!("destroying DEK for workspace '{}'", slug))?;

    if found {
        println!("workspace '{}': DEK destroyed. encrypted data is now unreadable.", slug);
    } else {
        anyhow::bail!("workspace '{}' not found", slug);
    }

    Ok(())
}
