use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use uuid::Uuid;

use maranode_common::user::{AuthProvider, Role, User};
use maranode_store::UserDb;

#[derive(Subcommand)]
pub enum UsersCommand {
    /// list all users in database
    List,
    /// create new local user account
    Create {
        /// login username
        username: String,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long, default_value = "viewer")]
        role: String,
    },
    /// change password for local user
    SetPassword {
        username: String,
        #[arg(long)]
        password: Option<String>,
    },
    /// disable user login. user record stays for audit
    Disable {
        username: String,
    },
    /// enable user that was disabled before
    Enable {
        username: String,
    },
    /// delete user record permanently
    Delete {
        username: String,
    },
}

pub fn run(cmd: UsersCommand, data_dir: &Path) -> Result<()> {
    let db_path = data_dir.join("users.db");
    if !db_path.exists() {
        anyhow::bail!(
            "user database not found at {}: is the daemon's data-dir correct?",
            db_path.display()
        );
    }
    let db = UserDb::open(&db_path).context("opening user database")?;

    match cmd {
        UsersCommand::List => cmd_list(&db),
        UsersCommand::Create {
            username,
            password,
            email,
            role,
        } => cmd_create(&db, &username, password, email, &role),
        UsersCommand::SetPassword { username, password } => {
            cmd_set_password(&db, &username, password)
        }
        UsersCommand::Disable { username } => cmd_set_active(&db, &username, false),
        UsersCommand::Enable { username } => cmd_set_active(&db, &username, true),
        UsersCommand::Delete { username } => cmd_delete(&db, &username),
    }
}

fn cmd_list(db: &UserDb) -> Result<()> {
    let users = db.list().context("listing users")?;
    if users.is_empty() {
        println!("{}", "no users".dimmed());
        return Ok(());
    }

    let col_user = 20usize;
    let col_role = 10usize;
    let col_prov = 8usize;
    let col_status = 8usize;

    println!(
        "{:<col_user$}  {:<col_role$}  {:<col_prov$}  {:<col_status$}  {}",
        "USERNAME".bold(),
        "ROLE".bold(),
        "PROVIDER".bold(),
        "STATUS".bold(),
        "LAST LOGIN".bold(),
    );

    for u in &users {
        let status = if u.active {
            "active".green().to_string()
        } else {
            "disabled".red().to_string()
        };
        let last = u
            .last_login
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "never".dimmed().to_string());

        println!(
            "{:<col_user$}  {:<col_role$}  {:<col_prov$}  {:<col_status$}  {}",
            u.username,
            u.role.as_str(),
            u.provider.as_str(),
            status,
            last,
        );
    }

    println!("\n{} user(s)", users.len());
    Ok(())
}

fn cmd_create(
    db: &UserDb,
    username: &str,
    password: Option<String>,
    email: Option<String>,
    role_str: &str,
) -> Result<()> {
    let role: Role = role_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    if db.get_by_username(username)?.is_some() {
        anyhow::bail!("user '{}' already exists", username);
    }

    let password = match password {
        Some(p) => p,
        None => prompt_password("password: ")?,
    };

    if password.len() < 8 {
        anyhow::bail!("password must be at least 8 characters");
    }

    let hash = UserDb::hash_password(&password).context("hashing password")?;

    let user = User {
        id: Uuid::new_v4(),
        username: username.to_string(),
        email,
        password_hash: Some(hash),
        role,
        provider: AuthProvider::Local,
        provider_sub: None,
        active: true,
        created_at: chrono::Utc::now(),
        last_login: None,
    };

    db.create(&user).context("creating user")?;

    println!(
        "{} created user {} ({})",
        "●".green().bold(),
        username.bold(),
        role_str,
    );
    Ok(())
}

fn cmd_set_password(db: &UserDb, username: &str, password: Option<String>) -> Result<()> {
    let mut user = db
        .get_by_username(username)?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found", username))?;

    if !user.is_local() {
        anyhow::bail!(
            "user '{}' authenticates via {}: password cannot be set",
            username,
            user.provider.as_str()
        );
    }

    let password = match password {
        Some(p) => p,
        None => prompt_password("new password: ")?,
    };

    if password.len() < 8 {
        anyhow::bail!("password must be at least 8 characters");
    }

    user.password_hash = Some(UserDb::hash_password(&password).context("hashing password")?);
    db.update(&user).context("updating user")?;

    println!(
        "{} password updated for {}",
        "●".green().bold(),
        username.bold()
    );
    Ok(())
}

fn cmd_set_active(db: &UserDb, username: &str, active: bool) -> Result<()> {
    let mut user = db
        .get_by_username(username)?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found", username))?;

    if user.active == active {
        let state = if active {
            "already enabled"
        } else {
            "already disabled"
        };
        println!("{} {} is {}", "●".yellow().bold(), username.bold(), state);
        return Ok(());
    }

    user.active = active;
    db.update(&user).context("updating user")?;

    let (symbol, verb) = if active {
        ("●".green().bold().to_string(), "enabled")
    } else {
        ("●".yellow().bold().to_string(), "disabled")
    };
    println!("{} {} {}", symbol, username.bold(), verb);
    Ok(())
}

fn cmd_delete(db: &UserDb, username: &str) -> Result<()> {
    let user = db
        .get_by_username(username)?
        .ok_or_else(|| anyhow::anyhow!("user '{}' not found", username))?;

    db.delete(user.id).context("deleting user")?;
    println!("{} deleted user {}", "●".green().bold(), username.bold());
    Ok(())
}

fn prompt_password(prompt: &str) -> Result<String> {
    eprint!("{}", prompt);
    let password = rpassword::read_password().context("reading password")?;
    Ok(password)
}
