//! maranode CLI binary (maranode)

mod commands;
mod errors;

use anyhow::Result;
use clap::{Parser, Subcommand};

use maranode_common::paths::default_data_dir;

#[derive(Parser)]
#[command(name = "maranode", about = "Maranode Privacy-first local AI runtime", version)]
struct Cli {
    /// daemon HTTP address
    #[arg(long, default_value = "http://127.0.0.1:11984", env = "MARANODE_HOST")]
    host: String,

    /// data directory path
    #[arg(long, env = "MARANODE_DATA_DIR")]
    data_dir: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// manage models in local store
    Model {
        #[command(subcommand)]
        action: commands::model::ModelCommand,
    },
    /// audit log commands
    Audit {
        #[command(subcommand)]
        action: commands::audit::AuditCommand,
    },
    /// health and integrity checks
    Verify {
        #[command(subcommand)]
        action: commands::verify::VerifyCommand,
    },
    /// send chat message and print response
    Chat {
        /// user message text
        prompt: String,
        /// model name:tag to use
        #[arg(long, default_value = "llama3.2:3b")]
        model: String,
        /// use RAG to add document context to answer
        #[arg(long)]
        rag: bool,
        /// RAG collection name
        #[arg(long)]
        collection: Option<String>,
    },
    /// manage local RAG document store
    Rag {
        #[command(subcommand)]
        action: commands::rag::RagCommand,
    },
    /// show daemon status and runtime stats
    Status,
    /// manage local users: list, create, password, disable
    Users {
        #[command(subcommand)]
        action: commands::users::UsersCommand,
    },
    /// admin operations. needs auth.admin_key when daemon has it set
    Admin {
        #[command(subcommand)]
        action: commands::admin::AdminCommand,
    },
    /// start runtime daemon by exec maranoded
    Serve {
        /// extra arguments passed to maranoded
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        daemon_args: Vec<String>,
    },
    /// manage workspaces (encryption, shredding)
    Workspace {
        #[command(subcommand)]
        action: commands::workspace::WorkspaceCommand,
    },
    /// model behavioral baseline: create, sign, verify, fetch, check
    Baseline {
        #[command(subcommand)]
        action: commands::baseline::BaselineCommand,
    },
    /// model approval registry: submit, approve, revoke, import/export tokens
    Registry {
        #[command(subcommand)]
        action: commands::registry::RegistryCommand,
    },
    /// DLP label sync from Purview, Forcepoint, or Symantec
    Dlp {
        #[command(subcommand)]
        action: commands::dlp::DlpCommand,
    },
    /// TPM key sealing: status, capture PCRs, seal, unseal-test, verify
    Tpm {
        #[command(subcommand)]
        action: commands::tpm::TpmCommand,
    },
    /// incident response: declare, investigate, resolve, forensic snapshot, break-glass
    Incident {
        #[command(subcommand)]
        action: commands::incident::IncidentCommand,
    },
    /// legal hold on audit segments: generate-key, place, release, sign-release, list
    Hold {
        #[command(subcommand)]
        action: commands::hold::HoldCommand,
    },
    /// scan local source files for common insecure patterns (heuristic, offline)
    Scan {
        /// file or directory to scan
        path: std::path::PathBuf,
        /// minimum severity to report: high, medium, low
        #[arg(long, default_value = "low")]
        min_severity: String,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("maranode=info")
        .with_target(false)
        .init();

    if let Err(e) = run().await {
        errors::print_cli_error(&e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = cli.data_dir.unwrap_or_else(default_data_dir);

    match cli.command {
        Commands::Model { action } => {
            commands::model::run(action, &data_dir).await?;
        }
        Commands::Audit { action } => {
            commands::audit::run(action, &data_dir, &cli.host).await?;
        }
        Commands::Verify { action } => {
            commands::verify::run(action, &cli.host).await?;
        }
        Commands::Chat {
            prompt,
            model,
            rag,
            collection,
        } => {
            let use_rag = rag || collection.is_some();
            commands::chat::run(&prompt, &model, &cli.host, use_rag, collection).await?;
        }
        Commands::Rag { action } => {
            commands::rag::run(action, &cli.host).await?;
        }
        Commands::Status => {
            commands::status::run(&cli.host).await?;
        }
        Commands::Users { action } => {
            commands::users::run(action, &data_dir)?;
        }
        Commands::Serve { daemon_args } => {
            commands::serve::run(&daemon_args)?;
        }
        Commands::Admin { action } => {
            let key = std::env::var("MARANODE_ADMIN_KEY").ok();
            match action {
                commands::admin::AdminCommand::ConfigReload => {
                    commands::admin::reload_config(&cli.host, key.as_deref()).await?;
                }
            }
        }
        Commands::Workspace { action } => {
            commands::workspace::run(action, &data_dir)?;
        }
        Commands::Baseline { action } => {
            commands::baseline::run(action, &data_dir, &cli.host).await?;
        }
        Commands::Registry { action } => {
            commands::registry::run(action, &data_dir, &cli.host).await?;
        }
        Commands::Dlp { action } => {
            commands::dlp::run(action, &cli.host).await?;
        }
        Commands::Tpm { action } => {
            commands::tpm::run(action, &data_dir).await?;
        }
        Commands::Incident { action } => {
            commands::incident::run(action, &cli.host).await?;
        }
        Commands::Hold { action } => {
            commands::hold::run(action, &cli.host).await?;
        }
        Commands::Scan { path, min_severity } => {
            commands::scan::run(&path, &min_severity)?;
        }
    }

    Ok(())
}
