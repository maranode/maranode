use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

impl Role {
    pub fn can_manage_users(&self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn can_manage_workspaces(&self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn can_manage_models(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    pub fn can_ingest_rag(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    pub fn can_view_audit(&self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    pub fn can_export_compliance(&self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn can_chat(&self) -> bool {
        true
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Viewer => "viewer",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "operator" => Ok(Role::Operator),
            "viewer" => Ok(Role::Viewer),
            other => Err(format!(
                "unknown role '{}': expected admin, operator, viewer",
                other
            )),
        }
    }
}

/// how the user signs in
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthProvider {
    Local,
    Oidc,
    Ldap,
    Saml,
}

impl AuthProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthProvider::Local => "local",
            AuthProvider::Oidc => "oidc",
            AuthProvider::Ldap => "ldap",
            AuthProvider::Saml => "saml",
        }
    }
}

impl std::str::FromStr for AuthProvider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "local" => Ok(AuthProvider::Local),
            "oidc" => Ok(AuthProvider::Oidc),
            "ldap" => Ok(AuthProvider::Ldap),
            "saml" => Ok(AuthProvider::Saml),
            other => Err(format!("unknown provider '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: Option<String>,
    /// argon2 password hash. none for SSO-only users
    pub password_hash: Option<String>,
    pub role: Role,
    pub provider: AuthProvider,
    /// external subject ID from OIDC sub, LDAP DN, or SAML NameID
    pub provider_sub: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

impl User {
    pub fn is_local(&self) -> bool {
        self.provider == AuthProvider::Local
    }
}
