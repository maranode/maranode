use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// a single capability that a role may hold. roles map to a set of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Chat,
    RagIngest,
    RagManage,
    ModelManage,
    AuditView,
    AuditExport,
    AuditPrune,
    UserManage,
    WorkspaceManage,
    IncidentManage,
    DlpManage,
    ConfigManage,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Chat => "chat",
            Permission::RagIngest => "rag_ingest",
            Permission::RagManage => "rag_manage",
            Permission::ModelManage => "model_manage",
            Permission::AuditView => "audit_view",
            Permission::AuditExport => "audit_export",
            Permission::AuditPrune => "audit_prune",
            Permission::UserManage => "user_manage",
            Permission::WorkspaceManage => "workspace_manage",
            Permission::IncidentManage => "incident_manage",
            Permission::DlpManage => "dlp_manage",
            Permission::ConfigManage => "config_manage",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Operator,
    Auditor,
    Viewer,
}

impl Role {
    /// the capabilities granted to this role. single source of truth for access checks.
    pub fn permissions(&self) -> &'static [Permission] {
        use Permission::*;
        match self {
            Role::Admin => &[
                Chat,
                RagIngest,
                RagManage,
                ModelManage,
                AuditView,
                AuditExport,
                AuditPrune,
                UserManage,
                WorkspaceManage,
                IncidentManage,
                DlpManage,
                ConfigManage,
            ],
            Role::Operator => &[Chat, RagIngest, RagManage, ModelManage, AuditView],
            Role::Auditor => &[Chat, AuditView, AuditExport],
            Role::Viewer => &[Chat],
        }
    }

    pub fn has(&self, perm: Permission) -> bool {
        self.permissions().contains(&perm)
    }

    pub fn can_manage_users(&self) -> bool {
        self.has(Permission::UserManage)
    }
    pub fn can_manage_workspaces(&self) -> bool {
        self.has(Permission::WorkspaceManage)
    }
    pub fn can_manage_models(&self) -> bool {
        self.has(Permission::ModelManage)
    }
    pub fn can_ingest_rag(&self) -> bool {
        self.has(Permission::RagIngest)
    }
    pub fn can_view_audit(&self) -> bool {
        self.has(Permission::AuditView)
    }
    pub fn can_export_compliance(&self) -> bool {
        self.has(Permission::AuditExport)
    }
    pub fn can_chat(&self) -> bool {
        self.has(Permission::Chat)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Auditor => "auditor",
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
            "auditor" => Ok(Role::Auditor),
            "viewer" => Ok(Role::Viewer),
            other => Err(format!(
                "unknown role '{}': expected admin, operator, auditor, viewer",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn admin_has_everything() {
        let r = Role::Admin;
        for p in [
            Permission::Chat,
            Permission::RagIngest,
            Permission::ModelManage,
            Permission::AuditExport,
            Permission::AuditPrune,
            Permission::UserManage,
            Permission::WorkspaceManage,
            Permission::ConfigManage,
        ] {
            assert!(r.has(p), "admin should have {:?}", p);
        }
    }

    #[test]
    fn operator_runs_but_does_not_export_or_prune() {
        let r = Role::Operator;
        assert!(r.has(Permission::ModelManage));
        assert!(r.has(Permission::RagIngest));
        assert!(r.has(Permission::AuditView));
        assert!(!r.has(Permission::AuditExport));
        assert!(!r.has(Permission::AuditPrune));
        assert!(!r.has(Permission::UserManage));
    }

    #[test]
    fn auditor_reviews_and_exports_only() {
        let r = Role::Auditor;
        assert!(r.has(Permission::AuditView));
        assert!(r.has(Permission::AuditExport));
        assert!(r.has(Permission::Chat));
        assert!(!r.has(Permission::AuditPrune));
        assert!(!r.has(Permission::RagIngest));
        assert!(!r.has(Permission::ModelManage));
        // the can_* wrappers must agree with has()
        assert!(r.can_export_compliance());
        assert!(!r.can_ingest_rag());
    }

    #[test]
    fn viewer_can_only_chat() {
        let r = Role::Viewer;
        assert_eq!(r.permissions(), &[Permission::Chat]);
    }

    #[test]
    fn role_string_roundtrip() {
        for r in [Role::Admin, Role::Operator, Role::Auditor, Role::Viewer] {
            assert_eq!(Role::from_str(r.as_str()), Ok(r.clone()));
        }
        assert!(Role::from_str("root").is_err());
    }
}
