//! single sign-on (sso) routes: oidc, ldap, saml

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use maranode_common::user::{AuthProvider, Role, User};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/auth/oidc/login", get(oidc_login))
        .route("/v1/auth/oidc/callback", get(oidc_callback))
        .route("/v1/auth/ldap/login", post(ldap_login))
        .route("/v1/auth/saml/login", get(saml_login))
        .route("/v1/auth/saml/callback", post(saml_callback))
        .route("/v1/auth/providers", get(list_providers))
}

#[derive(Serialize)]
struct ProvidersResp {
    local: bool,
    oidc: bool,
    ldap: bool,
    saml: bool,
}

async fn list_providers(State(state): State<AppState>) -> Json<ProvidersResp> {
    let identity = state.rt().identity;
    Json(ProvidersResp {
        local: true,
        oidc: identity.oidc.is_some(),
        #[cfg(feature = "ldap")]
        ldap: identity.ldap.is_some(),
        #[cfg(not(feature = "ldap"))]
        ldap: false,
        saml: identity.saml.is_some(),
    })
}

#[derive(Serialize)]
struct TokenResp {
    token: String,
}

async fn oidc_login(State(state): State<AppState>) -> ApiResult<Response> {
    use oauth2::{CsrfToken, PkceCodeChallenge, Scope};
    use openidconnect::core::CoreClient;
    use openidconnect::{IssuerUrl, RedirectUrl};

    let identity = state.rt().identity;
    let cfg = identity
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::not_found("OIDC is not configured"))?;

    let meta = openidconnect::core::CoreProviderMetadata::discover_async(
        IssuerUrl::new(cfg.issuer_url.clone()).map_err(|e| ApiError::internal(e.to_string()))?,
        openidconnect::reqwest::async_http_client,
    )
    .await
    .map_err(|e| ApiError::internal(format!("OIDC discovery: {}", e)))?;

    let client = CoreClient::from_provider_metadata(
        meta,
        openidconnect::ClientId::new(cfg.client_id.clone()),
        Some(openidconnect::ClientSecret::new(cfg.client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(cfg.redirect_uri.clone())
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );

    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, csrf_token, nonce) = client
        .authorize_url(
            openidconnect::AuthenticationFlow::<openidconnect::core::CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            openidconnect::Nonce::new_random,
        )
        .add_scope(Scope::new("openid".into()))
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    crate::state::oidc_pending_insert(
        &state.oidc_pending,
        csrf_token.secret().clone(),
        pkce_verifier.secret().clone(),
        nonce.secret().clone(),
    )
    .await;

    Ok(Redirect::temporary(auth_url.as_str()).into_response())
}

#[derive(Deserialize)]
struct OidcCallbackParams {
    code: String,
    state: Option<String>,
}

async fn oidc_callback(
    State(state): State<AppState>,
    Query(params): Query<OidcCallbackParams>,
) -> ApiResult<Json<TokenResp>> {
    use oauth2::PkceCodeVerifier;
    use openidconnect::core::CoreClient;
    use openidconnect::{AuthorizationCode, IssuerUrl, RedirectUrl, TokenResponse};

    let state_token = params
        .state
        .as_deref()
        .ok_or_else(|| ApiError::unauthorized("missing state parameter"))?;

    let pending =
        crate::state::oidc_pending_take(&state.oidc_pending, state_token)
            .await
            .ok_or_else(|| ApiError::unauthorized("invalid or expired OIDC state"))?;

    let identity = state.rt().identity;
    let cfg = identity
        .oidc
        .as_ref()
        .ok_or_else(|| ApiError::not_found("OIDC is not configured"))?;

    let meta = openidconnect::core::CoreProviderMetadata::discover_async(
        IssuerUrl::new(cfg.issuer_url.clone()).map_err(|e| ApiError::internal(e.to_string()))?,
        openidconnect::reqwest::async_http_client,
    )
    .await
    .map_err(|e| ApiError::internal(format!("OIDC discovery: {}", e)))?;

    let client = CoreClient::from_provider_metadata(
        meta,
        openidconnect::ClientId::new(cfg.client_id.clone()),
        Some(openidconnect::ClientSecret::new(cfg.client_secret.clone())),
    )
    .set_redirect_uri(
        RedirectUrl::new(cfg.redirect_uri.clone())
            .map_err(|e| ApiError::internal(e.to_string()))?,
    );

    let pkce_verifier = PkceCodeVerifier::new(pending.pkce_verifier_secret);
    let nonce = openidconnect::Nonce::new(pending.nonce_secret);

    let token_resp = client
        .exchange_code(AuthorizationCode::new(params.code))
        .set_pkce_verifier(pkce_verifier)
        .request_async(openidconnect::reqwest::async_http_client)
        .await
        .map_err(|e| ApiError::internal(format!("OIDC token exchange: {}", e)))?;

    let id_token = token_resp
        .id_token()
        .ok_or_else(|| ApiError::internal("no id_token in response"))?;

    let claims = id_token
        .claims(&client.id_token_verifier(), &nonce)
        .map_err(|e| ApiError::internal(format!("OIDC claims: {}", e)))?;

    let sub = claims.subject().to_string();
    let email = claims.email().map(|e| e.to_string());
    let username = claims
        .preferred_username()
        .map(|u| u.to_string())
        .or_else(|| email.clone())
        .unwrap_or_else(|| sub.clone());

    upsert_sso_user_and_session(
        &state,
        AuthProvider::Oidc,
        &sub,
        &username,
        email.as_deref(),
        &cfg.default_role,
    )
    .await
}

#[derive(Deserialize)]
struct LdapLoginReq {
    username: String,
    password: String,
}

#[cfg(feature = "ldap")]
async fn ldap_login(
    State(state): State<AppState>,
    Json(req): Json<LdapLoginReq>,
) -> ApiResult<Json<TokenResp>> {
    use ldap3::{LdapConnAsync, Scope, SearchEntry};

    let identity = state.rt().identity;
    let cfg = identity
        .ldap
        .as_ref()
        .ok_or_else(|| ApiError::not_found("LDAP is not configured"))?;

    let (conn, mut ldap) = LdapConnAsync::new(&cfg.url)
        .await
        .map_err(|e| ApiError::internal(format!("LDAP connect: {}", e)))?;
    ldap3::drive!(conn);

    ldap.simple_bind(&cfg.bind_dn, &cfg.bind_pw)
        .await
        .map_err(|e| ApiError::internal(format!("LDAP bind: {}", e)))?
        .success()
        .map_err(|e| ApiError::internal(format!("LDAP bind: {}", e)))?;

    let filter = format!("({}={})", cfg.uid_attr, ldap_escape(&req.username));
    let (entries, _) = ldap
        .search(
            &cfg.base_dn,
            Scope::Subtree,
            &filter,
            vec!["dn", "mail", "memberOf"],
        )
        .await
        .map_err(|e| ApiError::internal(format!("LDAP search: {}", e)))?
        .success()
        .map_err(|e| ApiError::internal(format!("LDAP search: {}", e)))?;

    let entry = entries
        .into_iter()
        .next()
        .ok_or_else(|| ApiError::unauthorized("invalid credentials"))?;
    let entry = SearchEntry::construct(entry);
    let user_dn = entry.dn.clone();

    ldap.simple_bind(&user_dn, &req.password)
        .await
        .map_err(|_| ApiError::unauthorized("invalid credentials"))?
        .success()
        .map_err(|_| ApiError::unauthorized("invalid credentials"))?;

    let member_of: Vec<String> = entry.attrs.get("memberOf").cloned().unwrap_or_default();
    let role = cfg
        .group_role_map
        .iter()
        .find(|(g, _)| member_of.iter().any(|m| m.eq_ignore_ascii_case(g)))
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| cfg.default_role.clone());

    let email = entry.attrs.get("mail").and_then(|v| v.first()).cloned();
    ldap.unbind().await.ok();

    upsert_sso_user_and_session(
        &state,
        AuthProvider::Ldap,
        &user_dn,
        &req.username,
        email.as_deref(),
        &role,
    )
    .await
}

#[cfg(not(feature = "ldap"))]
async fn ldap_login(
    State(_state): State<AppState>,
    Json(_req): Json<LdapLoginReq>,
) -> ApiResult<Json<TokenResp>> {
    Err(ApiError::not_implemented(
        "LDAP support was not compiled in; rebuild maranoded with --features ldap",
    ))
}

#[cfg(feature = "ldap")]
fn ldap_escape(s: &str) -> String {
    s.replace('\\', "\\5c")
        .replace('*', "\\2a")
        .replace('(', "\\28")
        .replace(')', "\\29")
        .replace('\0', "\\00")
}

async fn saml_login(State(state): State<AppState>) -> ApiResult<Response> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use flate2::{write::DeflateEncoder, Compression};
    use std::io::Write;

    let identity = state.rt().identity;
    let cfg = identity
        .saml
        .as_ref()
        .ok_or_else(|| ApiError::not_found("SAML is not configured"))?;

    let sso_url = saml_idp_sso_url(&cfg.idp_metadata_url).await?;

    let id = format!("_{}", Uuid::new_v4().to_string().replace('-', ""));
    let now = Utc::now().to_rfc3339();
    let xml = format!(
        r#"<samlp:AuthnRequest xmlns:samlp="urn:oasis:names:tc:SAML:2.0:protocol" xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion" ID="{id}" Version="2.0" IssueInstant="{now}" Destination="{sso}" AssertionConsumerServiceURL="{acs}" ProtocolBinding="urn:oasis:names:tc:SAML:2.0:bindings:HTTP-POST"><saml:Issuer>{entity}</saml:Issuer></samlp:AuthnRequest>"#,
        id = id,
        now = now,
        sso = sso_url,
        acs = format!(
            "{}/v1/auth/saml/callback",
            base_url_from_entity(&cfg.sp_entity_id)
        ),
        entity = cfg.sp_entity_id,
    );

    let mut deflater = DeflateEncoder::new(Vec::new(), Compression::default());
    deflater
        .write_all(xml.as_bytes())
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let compressed = deflater
        .finish()
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let encoded = STANDARD.encode(&compressed);
    let redirect = format!("{}?SAMLRequest={}", sso_url, urlencoding::encode(&encoded));

    Ok(Redirect::temporary(&redirect).into_response())
}

#[derive(Deserialize)]
struct SamlCallbackForm {
    #[serde(rename = "SAMLResponse")]
    saml_response: String,
    #[serde(rename = "RelayState")]
    #[allow(dead_code)]
    relay_state: Option<String>,
}

async fn saml_callback(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<SamlCallbackForm>,
) -> ApiResult<Json<TokenResp>> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let identity = state.rt().identity;
    let cfg = identity
        .saml
        .as_ref()
        .ok_or_else(|| ApiError::not_found("SAML is not configured"))?;

    let xml_bytes = STANDARD
        .decode(&form.saml_response)
        .map_err(|e| ApiError::unauthorized(format!("invalid SAMLResponse: {}", e)))?;
    let xml = String::from_utf8(xml_bytes)
        .map_err(|_| ApiError::unauthorized("SAMLResponse is not valid UTF-8"))?;

    let (subject, email, username) = saml_parse_assertion(&xml)
        .map_err(|e| ApiError::unauthorized(format!("SAML assertion: {}", e)))?;

    upsert_sso_user_and_session(
        &state,
        AuthProvider::Saml,
        &subject,
        &username,
        email.as_deref(),
        &cfg.default_role,
    )
    .await
}

/// download idp metadata xml and read sso redirect url from it
async fn saml_idp_sso_url(metadata_url: &str) -> ApiResult<String> {
    let xml = reqwest::get(metadata_url)
        .await
        .map_err(|e| ApiError::internal(format!("fetching IdP metadata: {}", e)))?
        .text()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let mut reader = quick_xml::Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Empty(e)) | Ok(quick_xml::events::Event::Start(e))
                if e.local_name().as_ref() == b"SingleSignOnService" =>
            {
                let binding = e
                    .attributes()
                    .filter_map(|a| a.ok())
                    .find(|a| a.key.local_name().as_ref() == b"Binding")
                    .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                    .unwrap_or_default();
                if binding.contains("HTTP-Redirect") {
                    if let Some(loc) = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.local_name().as_ref() == b"Location")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                    {
                        return Ok(loc);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(ApiError::internal(format!("parsing IdP metadata: {}", e))),
            _ => {}
        }
    }
    Err(ApiError::internal(
        "no HTTP-Redirect SSO URL found in IdP metadata",
    ))
}

/// read nameid, email, username from samlresponse xml string
fn saml_parse_assertion(xml: &str) -> Result<(String, Option<String>, String), String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut name_id = String::new();
    let mut email: Option<String> = None;
    let mut username: Option<String> = None;
    let mut in_name_id = false;
    let mut current_attr_name = String::new();
    let mut in_attr_value = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => match e.local_name().as_ref() {
                b"NameID" => {
                    in_name_id = true;
                }
                b"Attribute" => {
                    current_attr_name = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .find(|a| a.key.local_name().as_ref() == b"Name")
                        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
                        .unwrap_or_default();
                }
                b"AttributeValue" => {
                    in_attr_value = true;
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::End(e)) => match e.local_name().as_ref() {
                b"NameID" => {
                    in_name_id = false;
                }
                b"AttributeValue" => {
                    in_attr_value = false;
                }
                _ => {}
            },
            Ok(quick_xml::events::Event::Text(t)) => {
                let text = t.unescape().unwrap_or_default().to_string();
                if in_name_id && !text.is_empty() {
                    name_id = text.clone();
                }
                if in_attr_value {
                    let name_lower = current_attr_name.to_lowercase();
                    if name_lower.contains("email") || name_lower.contains("mail") {
                        email = Some(text.clone());
                    }
                    if name_lower.contains("uid")
                        || name_lower.contains("username")
                        || name_lower.contains("login")
                        || name_lower.contains("samaccountname")
                    {
                        username = Some(text);
                    }
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
    }

    if name_id.is_empty() {
        return Err("no NameID found in assertion".into());
    }

    let resolved_username = username
        .or_else(|| email.clone())
        .unwrap_or_else(|| name_id.clone());
    Ok((name_id, email, resolved_username))
}

fn base_url_from_entity(entity_id: &str) -> String {
    if let Ok(u) = url::Url::parse(entity_id) {
        format!("{}://{}", u.scheme(), u.host_str().unwrap_or("localhost"))
    } else {
        entity_id.to_string()
    }
}

async fn upsert_sso_user_and_session(
    state: &AppState,
    provider: AuthProvider,
    sub: &str,
    username: &str,
    email: Option<&str>,
    role_str: &str,
) -> ApiResult<Json<TokenResp>> {
    let role: Role = role_str
        .parse()
        .map_err(|e: String| ApiError::bad_request(e))?;
    let db = state.user_db.lock().await;

    let user = match db
        .get_by_provider_sub(provider.as_str(), sub)
        .map_err(|e| ApiError::internal(e.to_string()))?
    {
        Some(mut existing) => {
            if let Some(e) = email {
                existing.email = Some(e.to_string());
            }
            db.update(&existing)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            existing
        }
        None => {
            let u = User {
                id: Uuid::new_v4(),
                username: username.to_string(),
                email: email.map(str::to_string),
                password_hash: None,
                role,
                provider: provider.clone(),
                provider_sub: Some(sub.to_string()),
                active: true,
                created_at: Utc::now(),
                last_login: None,
            };
            db.create(&u)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            u
        }
    };

    if !user.active {
        return Err(ApiError::forbidden("account is disabled"));
    }

    let token = db
        .create_session(user.id, state.rt().identity.session_hours)
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(TokenResp { token }))
}
