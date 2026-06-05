# Users & Access Control

Maranode has a built-in user system with local accounts, role-based access control, and support for external identity providers (OIDC, LDAP/Active Directory, SAML 2.0).

## Roles

| Role | Capabilities |
|---|---|
| `admin` | Full access - manage users, workspaces, models, audit, compliance exports |
| `operator` | Chat, RAG ingest, view audit log, manage models |
| `viewer` | Chat only |

## First-run bootstrap

On first start, if `auth.admin_key` is set in config and the user database is empty, Maranode creates an initial `admin` user with the admin key as password. Change the password immediately after first login.

```toml
[auth]
admin_key = "your-secret-key"
```

If no admin key is set, the database starts empty and the admin key bypasses user auth for direct API access.

## Web UI sign-in

The web UI shows a login page on first visit. Enter your username and password, or click an SSO button if a provider is configured. The session persists in your browser until you sign out.

After sign-in, the **Users** page (sidebar -> Users) lets admins create, edit, disable, and delete accounts.

## Local user management

### CLI

The `maranode users` subcommand manages users directly from the local data directory - no daemon required, no HTTP. Useful for initial provisioning, disaster recovery, or scripted setup.

```bash
# list all users
maranode users list

# create a user (password prompted interactively)
maranode users create jane --role operator
maranode users create jane --role operator --email jane@example.com

# supply password non-interactively (e.g. in provisioning scripts)
maranode users create jane --role operator --password s3cr3t

# change a user's password (prompted if --password omitted)
maranode users set-password jane

# disable a user - login blocked, audit trail preserved
maranode users disable jane

# re-enable
maranode users enable jane

# permanently delete
maranode users delete jane
```

`set-password` refuses SSO accounts (OIDC/LDAP/SAML) since their passwords are managed by the external provider.

If the data directory is not the default, pass `--data-dir`:

```bash
maranode --data-dir /opt/maranode/data users list
```

### HTTP API

**Create a user:**

```bash
curl -X POST http://localhost:11984/v1/users \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"username":"jane","password":"s3cr3t","email":"jane@example.com","role":"operator"}'
```

**List users:**

```bash
curl http://localhost:11984/v1/users \
  -H "Authorization: Bearer $ADMIN_KEY"
```

**Update a user:**

```bash
curl -X PUT http://localhost:11984/v1/users/<id> \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"role":"admin","active":true}'
```

**Change password:**

```bash
curl -X PUT http://localhost:11984/v1/users/<id>/password \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"password":"newpassword"}'
```

**Delete a user:**

```bash
curl -X DELETE http://localhost:11984/v1/users/<id> \
  -H "Authorization: Bearer $ADMIN_KEY"
```

## Authentication (login / logout)

```bash
# Login - returns a session token
curl -X POST http://localhost:11984/v1/auth/login \
  -H "Content-Type: application/json" \
  -d '{"username":"jane","password":"s3cr3t"}'
# -> {"token":"<token>","user":{...}}

# Use the token
curl http://localhost:11984/v1/chat/completions \
  -H "Authorization: Bearer <token>" \
  ...

# Logout
curl -X POST http://localhost:11984/v1/auth/logout \
  -H "Authorization: Bearer <token>"

# Get current user
curl http://localhost:11984/v1/auth/me \
  -H "Authorization: Bearer <token>"
```

Sessions expire after 24 hours by default. Configure with `auth.session_hours`.

## OIDC (Google, Microsoft, Okta, etc.)

Add to `config.toml`:

```toml
[auth.oidc]
issuer_url    = "https://accounts.google.com"
client_id     = "your-client-id"
client_secret = "your-client-secret"
redirect_uri  = "http://localhost:11984/v1/auth/oidc/callback"
default_role  = "viewer"
```

Register `redirect_uri` in your identity provider's OAuth app settings. On login:

1. Browser navigates to `GET /v1/auth/oidc/login` -> redirected to provider
2. User authenticates with the provider
3. Provider redirects to `/v1/auth/oidc/callback?code=...`
4. Maranode exchanges the code, creates or updates the local user, returns a session token

First login auto-creates a user with `default_role`. Subsequent logins update the email if changed.

## LDAP / Active Directory

```toml
[auth.ldap]
url      = "ldaps://dc.example.com:636"
bind_dn  = "cn=svc-maranode,dc=example,dc=com"
bind_pw  = "service-account-password"
base_dn  = "ou=Users,dc=example,dc=com"
uid_attr = "sAMAccountName"   # or "uid" for OpenLDAP
default_role = "viewer"

[[auth.ldap.group_role_map]]
group_dn = "CN=Maranode-Admins,OU=Groups,DC=example,DC=com"
role     = "admin"

[[auth.ldap.group_role_map]]
group_dn = "CN=Maranode-Operators,OU=Groups,DC=example,DC=com"
role     = "operator"
```

LDAP login uses the local form (username + password). Maranode binds with the service account to find the user DN, then rebinds as the user to verify the password. Group membership determines the role (first match wins).

```bash
curl -X POST http://localhost:11984/v1/auth/ldap/login \
  -H "Content-Type: application/json" \
  -d '{"username":"jane","password":"s3cr3t"}'
```

## SAML 2.0

```toml
[auth.saml]
idp_metadata_url = "https://idp.example.com/metadata.xml"
sp_entity_id     = "https://maranode.example.com"
default_role     = "viewer"
# Optional: sign AuthnRequests
# sp_cert = "/etc/maranode/sp.crt"
# sp_key  = "/etc/maranode/sp.key"
```

Configure your IdP with:
- **ACS URL** (assertion consumer service): `http://localhost:11984/v1/auth/saml/callback`
- **Entity ID**: your `sp_entity_id`

Login flow:
1. Navigate to `GET /v1/auth/saml/login` -> redirected to IdP with AuthnRequest
2. User authenticates with IdP
3. IdP POSTs to `/v1/auth/saml/callback` with SAMLResponse
4. Maranode validates the assertion, creates or updates the user, returns a session token

The user's `uid` or `username` attribute is used as the Maranode username; `email` is mapped from the `email`/`mail` attribute.

## Provider availability

```bash
curl http://localhost:11984/v1/auth/providers
# -> {"local":true,"oidc":false,"ldap":true,"saml":false}
```

## Session configuration

```toml
[auth]
session_hours = 24   # how long a session token remains valid
```

Passwords are hashed with Argon2id. Session tokens are 64-character random hex strings stored in the local SQLite database (`<data-dir>/users.db`).
