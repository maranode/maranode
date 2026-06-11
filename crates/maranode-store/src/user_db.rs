use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use maranode_common::user::{AuthProvider, Role, User};

pub struct UserDb {
    conn: Connection,
}

impl UserDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("opening user db at {}", path.display()))?;
        conn.execute_batch(include_str!("sql/migrate_users.sql"))?;
        Ok(Self { conn })
    }

    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))? as u64)
    }

    pub fn create(&self, user: &User) -> Result<()> {
        self.conn.execute(
            "INSERT INTO users (id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                user.id.to_string(),
                user.username,
                user.email,
                user.password_hash,
                user.role.as_str(),
                user.provider.as_str(),
                user.provider_sub,
                user.active as i64,
                user.created_at.to_rfc3339(),
                user.last_login.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login FROM users ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], row_to_user)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn get_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login FROM users WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_user)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_by_email(&self, email: &str) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login FROM users WHERE email = ?1"
        )?;
        let mut rows = stmt.query_map(params![email], row_to_user)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_by_username(&self, username: &str) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login FROM users WHERE username = ?1"
        )?;
        let mut rows = stmt.query_map(params![username], row_to_user)?;
        Ok(rows.next().transpose()?)
    }

    pub fn get_by_provider_sub(&self, provider: &str, sub: &str) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, email, password_hash, role, provider, provider_sub, active, created_at, last_login FROM users WHERE provider = ?1 AND provider_sub = ?2"
        )?;
        let mut rows = stmt.query_map(params![provider, sub], row_to_user)?;
        Ok(rows.next().transpose()?)
    }

    pub fn update(&self, user: &User) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET username=?2, email=?3, password_hash=?4, role=?5, provider=?6, provider_sub=?7, active=?8, last_login=?9 WHERE id=?1",
            params![
                user.id.to_string(),
                user.username,
                user.email,
                user.password_hash,
                user.role.as_str(),
                user.provider.as_str(),
                user.provider_sub,
                user.active as i64,
                user.last_login.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: Uuid) -> Result<bool> {
        let n = self
            .conn
            .execute("DELETE FROM users WHERE id = ?1", params![id.to_string()])?;
        Ok(n > 0)
    }

    pub fn hash_password(password: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("password hash error: {}", e))?;
        Ok(hash.to_string())
    }

    pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
        let parsed = PasswordHash::new(hash).map_err(|e| anyhow::anyhow!("invalid hash: {}", e))?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    }

    // session tokens

    pub fn create_session(&self, user_id: Uuid, ttl_hours: i64) -> Result<String> {
        // delete expired sessions for this user before creating a new one
        self.conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND expires_at < ?2",
            params![user_id.to_string(), Utc::now().to_rfc3339()],
        )?;

        let token = Uuid::new_v4().to_string().replace('-', "")
            + &Uuid::new_v4().to_string().replace('-', "");
        let now = Utc::now();
        let expires = now + Duration::hours(ttl_hours);

        self.conn.execute(
            "INSERT INTO sessions (token, user_id, created_at, expires_at) VALUES (?1,?2,?3,?4)",
            params![
                token,
                user_id.to_string(),
                now.to_rfc3339(),
                expires.to_rfc3339()
            ],
        )?;

        // set last_login timestamp on the user row
        self.conn.execute(
            "UPDATE users SET last_login = ?1 WHERE id = ?2",
            params![now.to_rfc3339(), user_id.to_string()],
        )?;

        Ok(token)
    }

    pub fn resolve_session(&self, token: &str) -> Result<Option<User>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.username, u.email, u.password_hash, u.role, u.provider, u.provider_sub, u.active, u.created_at, u.last_login
             FROM sessions s JOIN users u ON s.user_id = u.id
             WHERE s.token = ?1 AND s.expires_at > ?2 AND u.active = 1",
        )?;
        let mut rows = stmt.query_map(params![token, now], row_to_user)?;
        Ok(rows.next().transpose()?)
    }

    pub fn delete_session(&self, token: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
        Ok(())
    }

    pub fn list_sessions_for_user(&self, user_id: Uuid) -> Result<Vec<SessionRecord>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT token, user_id, created_at, expires_at FROM sessions
             WHERE user_id = ?1 AND expires_at > ?2 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![user_id.to_string(), now], row_to_session)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn list_all_sessions(&self) -> Result<Vec<SessionRecord>> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT s.token, s.user_id, s.created_at, s.expires_at, u.username
             FROM sessions s JOIN users u ON s.user_id = u.id
             WHERE s.expires_at > ?1 ORDER BY s.created_at DESC",
        )?;
        let rows = stmt.query_map(params![now], |row| {
            let mut rec = row_to_session(row)?;
            rec.username = row.get(4)?;
            Ok(rec)
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
    }

    pub fn delete_session_by_prefix(&self, user_id: &Uuid, prefix: &str) -> Result<()> {
        let pattern = format!("{}%", prefix);
        self.conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token LIKE ?2",
            params![user_id.to_string(), pattern],
        )?;
        Ok(())
    }

    pub fn delete_sessions_for_user_except(&self, user_id: Uuid, keep_token: &str) -> Result<u64> {
        let n = self.conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1 AND token != ?2",
            params![user_id.to_string(), keep_token],
        )?;
        Ok(n as u64)
    }

    pub fn create_reset_token(&self, user_id: Uuid) -> Result<String> {
        self.conn.execute(
            "DELETE FROM password_reset_tokens WHERE expires_at < ?1",
            params![Utc::now().to_rfc3339()],
        )?;
        let token = Uuid::new_v4().to_string().replace('-', "")
            + &Uuid::new_v4().to_string().replace('-', "");
        let expires = Utc::now() + Duration::minutes(30);
        self.conn.execute(
            "INSERT INTO password_reset_tokens (token, user_id, expires_at) VALUES (?1,?2,?3)",
            params![token, user_id.to_string(), expires.to_rfc3339()],
        )?;
        Ok(token)
    }

    pub fn consume_reset_token(&self, token: &str) -> Result<Option<Uuid>> {
        let now = Utc::now().to_rfc3339();
        let result = self.conn.query_row(
            "SELECT user_id FROM password_reset_tokens WHERE token = ?1 AND expires_at > ?2",
            params![token, now],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(uid_str) => {
                self.conn.execute(
                    "DELETE FROM password_reset_tokens WHERE token = ?1",
                    params![token],
                )?;
                Ok(Uuid::parse_str(&uid_str).ok())
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub token_prefix: String,
    pub user_id: Uuid,
    pub username: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    let token: String = row.get(0)?;
    let user_id_str: String = row.get(1)?;
    let created_str: String = row.get(2)?;
    let expires_str: String = row.get(3)?;

    Ok(SessionRecord {
        token_prefix: token.chars().take(8).collect(),
        user_id: Uuid::parse_str(&user_id_str).unwrap_or_else(|_| Uuid::nil()),
        username: None,
        created_at: DateTime::parse_from_rfc3339(&created_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        expires_at: DateTime::parse_from_rfc3339(&expires_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
    })
}

fn row_to_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    let role_str: String = row.get(4)?;
    let provider_str: String = row.get(5)?;
    let active: i64 = row.get(7)?;
    let created_str: String = row.get(8)?;
    let last_str: Option<String> = row.get(9)?;

    Ok(User {
        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_else(|_| Uuid::nil()),
        username: row.get(1)?,
        email: row.get(2)?,
        password_hash: row.get(3)?,
        role: Role::from_str(&role_str).unwrap_or(Role::Viewer),
        provider: AuthProvider::from_str(&provider_str).unwrap_or(AuthProvider::Local),
        provider_sub: row.get(6)?,
        active: active != 0,
        created_at: DateTime::parse_from_rfc3339(&created_str)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now()),
        last_login: last_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)),
    })
}
