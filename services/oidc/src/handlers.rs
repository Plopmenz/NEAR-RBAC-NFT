//! HTTP routes for the OIDC provider.

use std::str::FromStr;

use axum::extract::{Form, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use near_account_id::AccountId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;

use crate::config::{ClientConfig, Config};
use crate::error::AppError;
use crate::html;
use crate::nep413;
use crate::state::AppState;
use crate::store::{random_nonce, random_token, AuthorizationCode, PendingLogin, Session};
use crate::tokens;

/// Builds the provider router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(health))
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/.well-known/oauth-authorization-server", get(discovery))
        .route("/jwks.json", get(jwks))
        .route("/authorize", get(authorize))
        .route("/login", post(login))
        .route("/token", post(token))
        .route("/userinfo", get(userinfo).post(userinfo))
        .route("/introspect", post(introspect))
        .route("/logout", post(logout))
        .with_state(state)
}

// -------------------------------------------------------------------------
// Discovery and keys
// -------------------------------------------------------------------------

async fn index(State(state): State<AppState>) -> Html<String> {
    Html(html::index_page(&state.config))
}

async fn health() -> ([(header::HeaderName, &'static str); 1], &'static str) {
    ([(header::CACHE_CONTROL, "no-store")], "ok")
}

async fn discovery(State(state): State<AppState>) -> Json<Value> {
    let config = &state.config;
    Json(json!({
        "issuer": config.base_url(),
        "authorization_endpoint": config.endpoint("/authorize"),
        "token_endpoint": config.endpoint("/token"),
        "userinfo_endpoint": config.endpoint("/userinfo"),
        "jwks_uri": config.endpoint("/jwks.json"),
        "introspection_endpoint": config.endpoint("/introspect"),
        "end_session_endpoint": config.endpoint("/logout"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
        "scopes_supported": config.allowed_scopes,
        "token_endpoint_auth_methods_supported": [
            "none",
            "client_secret_post",
            "client_secret_basic"
        ],
        "introspection_endpoint_auth_methods_supported": [
            "none",
            "client_secret_post",
            "client_secret_basic"
        ],
        "code_challenge_methods_supported": ["S256"],
        "claims_supported": [
            "iss",
            "sub",
            "aud",
            "exp",
            "iat",
            "auth_time",
            "nonce",
            "preferred_username",
            "roles"
        ],
    }))
}

async fn jwks(State(state): State<AppState>) -> Json<crate::keys::Jwks> {
    Json(state.keys.jwks().clone())
}

// -------------------------------------------------------------------------
// Authorization and login
// -------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct AuthorizeParams {
    response_type: String,
    client_id: String,
    redirect_uri: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    code_challenge: String,
    code_challenge_method: String,
}

async fn authorize(
    State(state): State<AppState>,
    Query(params): Query<AuthorizeParams>,
) -> Result<Response, AppError> {
    let client = state
        .config
        .client(&params.client_id)
        .ok_or_else(|| AppError::invalid_request("unknown client_id"))?;

    if !client
        .redirect_uris
        .iter()
        .any(|uri| uri == &params.redirect_uri)
    {
        return Err(AppError::invalid_request(
            "redirect_uri is not registered for this client",
        ));
    }
    if params.response_type != "code" {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "unsupported_response_type",
            "only response_type=code is supported",
        ));
    }
    validate_pkce_challenge(&params.code_challenge, &params.code_challenge_method)?;
    let scope = normalize_scope(params.scope.as_deref(), &state.config, client)?;

    let now = state.clock.now();
    let pending = PendingLogin {
        id: random_token(),
        client_id: client.client_id.clone(),
        redirect_uri: params.redirect_uri,
        state: params.state,
        scope,
        oidc_nonce: params.nonce,
        code_challenge: params.code_challenge,
        nep413_nonce: random_nonce(),
        message: login_message(&state.config, client),
        created_at: now,
        expires_at: now + state.config.pending_login_ttl_secs,
    };
    if !state.store.insert_pending(pending.clone()).await {
        return Err(AppError::too_many_requests(
            "too many pending login requests; retry later",
        ));
    }

    Ok(Html(html::login_page(&state.config, client, &pending)).into_response())
}

fn login_message(config: &Config, client: &ClientConfig) -> String {
    format!(
        "{} wants you to sign in with your NEAR account.\n\nClient: {}\nIssuer: {}",
        client.display_name(),
        client.client_id,
        config.base_url()
    )
}

#[derive(Debug, Deserialize)]
struct LoginForm {
    request_id: String,
    account_id: String,
    public_key: String,
    signature: String,
}

async fn login(
    State(state): State<AppState>,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let now = state.clock.now();
    let pending = state
        .store
        .get_pending(&form.request_id)
        .await
        .ok_or_else(|| AppError::invalid_request("unknown or expired login request"))?;
    if now >= pending.expires_at {
        state.store.remove_pending(&pending.id).await;
        return Err(AppError::invalid_request("login request has expired"));
    }

    let account_id = AccountId::from_str(&form.account_id)
        .map_err(|_| AppError::invalid_request("invalid NEAR account id"))?;
    let account_id = account_id.to_string();

    nep413::verify(
        &form.public_key,
        &form.signature,
        &pending.message,
        pending.nep413_nonce,
        &state.config.recipient(),
        state.config.nep413_callback_url(),
    )
    .map_err(|err| {
        AppError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            err.to_string(),
        )
    })?;

    if state.config.verify_access_key
        && !state
            .roles
            .has_access_key(&account_id, &form.public_key)
            .await
            .map_err(|err| AppError::internal(err.to_string()))?
    {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_signature",
            "the public key is not an access key of the claimed account",
        ));
    }

    let roles = state
        .roles
        .roles_of(&account_id)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let client = state
        .config
        .client(&pending.client_id)
        .ok_or_else(|| AppError::internal("pending login references an unknown client"))?;
    let missing = missing_required_roles(client, &roles);
    if !missing.is_empty() {
        return Err(AppError::access_denied(format!(
            "account {} is missing required roles: {}",
            account_id,
            missing.join(", ")
        )));
    }

    let code = random_token();
    state
        .store
        .insert_code(
            code.clone(),
            AuthorizationCode {
                client_id: pending.client_id.clone(),
                redirect_uri: pending.redirect_uri.clone(),
                account_id,
                roles,
                scope: pending.scope.clone(),
                oidc_nonce: pending.oidc_nonce.clone(),
                code_challenge: pending.code_challenge.clone(),
                auth_time: now,
                expires_at: now + state.config.authorization_code_ttl_secs,
            },
        )
        .await;
    state.store.remove_pending(&pending.id).await;

    let location = redirect_with_code(&pending.redirect_uri, &code, pending.state.as_deref())?;
    Ok(Redirect::to(&location).into_response())
}

// -------------------------------------------------------------------------
// Token exchange
// -------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenForm {
    grant_type: String,
    code: String,
    redirect_uri: String,
    code_verifier: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    id_token: String,
    scope: String,
}

async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<TokenForm>,
) -> Result<Response, AppError> {
    let client = authenticate_client(
        &state.config,
        &headers,
        form.client_id.as_deref(),
        form.client_secret.as_deref(),
    )?;
    if form.grant_type != "authorization_code" {
        return Err(AppError::unsupported_grant_type(
            "only grant_type=authorization_code is supported",
        ));
    }

    let code =
        state.store.take_code(&form.code).await.ok_or_else(|| {
            AppError::invalid_grant("authorization code is invalid or already used")
        })?;
    let now = state.clock.now();
    if now >= code.expires_at {
        return Err(AppError::invalid_grant("authorization code has expired"));
    }
    if code.client_id != client.client_id {
        return Err(AppError::invalid_grant(
            "authorization code was issued to another client",
        ));
    }
    if code.redirect_uri != form.redirect_uri {
        return Err(AppError::invalid_grant("redirect_uri does not match"));
    }
    if !verify_pkce(&form.code_verifier, &code.code_challenge) {
        return Err(AppError::invalid_grant("PKCE verification failed"));
    }

    let session = Session {
        id: random_token(),
        account_id: code.account_id,
        client_id: code.client_id,
        roles: code.roles,
        scope: code.scope,
        auth_time: code.auth_time,
        issued_at: now,
        expires_at: now + state.config.access_token_ttl_secs,
        revoked: false,
    };
    let access_token =
        tokens::encode_access(&state.keys, &state.config, state.clock.as_ref(), &session)
            .map_err(|err| AppError::internal(err.to_string()))?;
    let id_token = tokens::encode_id(
        &state.keys,
        &state.config,
        state.clock.as_ref(),
        &session,
        code.oidc_nonce,
    )
    .map_err(|err| AppError::internal(err.to_string()))?;
    state.store.insert_session(session.clone()).await;

    Ok((
        no_store(),
        Json(TokenResponse {
            access_token,
            token_type: "Bearer",
            expires_in: state.config.access_token_ttl_secs,
            id_token,
            scope: session.scope,
        }),
    )
        .into_response())
}

// -------------------------------------------------------------------------
// Userinfo
// -------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct UserInfo {
    sub: String,
    preferred_username: String,
    roles: Vec<String>,
}

async fn userinfo(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let token = bearer_token(&headers)?;
    let claims = tokens::decode_access(&state.keys, &state.config, &token)
        .map_err(|_| AppError::unauthorized("invalid access token"))?;
    let session = active_session(&state, &claims.jti, &claims.sub).await?;
    let roles = ensure_live_roles(&state, &session).await?;

    Ok((
        no_store(),
        Json(UserInfo {
            sub: session.account_id.clone(),
            preferred_username: session.account_id,
            roles,
        }),
    )
        .into_response())
}

// -------------------------------------------------------------------------
// Introspection
// -------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IntrospectForm {
    token: String,
    #[serde(default)]
    token_type_hint: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}

async fn introspect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<IntrospectForm>,
) -> Result<Response, AppError> {
    let _ = form.token_type_hint;
    let authenticated_client = if form.client_id.is_some() || has_basic_auth(&headers) {
        Some(authenticate_client(
            &state.config,
            &headers,
            form.client_id.as_deref(),
            form.client_secret.as_deref(),
        )?)
    } else {
        None
    };

    let claims = match tokens::decode_access(&state.keys, &state.config, &form.token) {
        Ok(claims) => claims,
        Err(_) => return Ok(Json(json!({ "active": false })).into_response()),
    };
    let session = match state.store.get_session(&claims.jti).await {
        Some(session)
            if !session.revoked
                && session.account_id == claims.sub
                && session.client_id == claims.client_id
                && state.clock.now() < session.expires_at =>
        {
            session
        }
        _ => return Ok(Json(json!({ "active": false })).into_response()),
    };

    let client = state
        .config
        .client(&session.client_id)
        .ok_or_else(|| AppError::internal("session references an unknown client"))?;
    if let Some(authenticated) = authenticated_client {
        if authenticated.client_id != client.client_id {
            return Ok(Json(json!({ "active": false })).into_response());
        }
    } else if client.client_secret.is_some() {
        return Err(AppError::invalid_client(
            "client authentication is required for this token",
        ));
    }

    let roles = state
        .roles
        .roles_of(&session.account_id)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    if !missing_required_roles(client, &roles).is_empty() {
        state.store.revoke_session(&session.id).await;
        return Ok(Json(json!({ "active": false })).into_response());
    }

    Ok((
        no_store(),
        Json(json!({
            "active": true,
            "iss": claims.iss,
            "sub": claims.sub,
            "aud": claims.aud,
            "client_id": claims.client_id,
            "scope": claims.scope,
            "username": claims.sub,
            "token_type": "Bearer",
            "exp": claims.exp,
            "iat": claims.iat,
            "roles": roles,
        })),
    )
        .into_response())
}

// -------------------------------------------------------------------------
// Logout
// -------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LogoutForm {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    id_token_hint: Option<String>,
}

async fn logout(
    State(state): State<AppState>,
    Form(form): Form<LogoutForm>,
) -> Result<Response, AppError> {
    let token = form
        .token
        .or(form.id_token_hint)
        .ok_or_else(|| AppError::invalid_request("token or id_token_hint is required"))?;

    let session_id = tokens::decode_access(&state.keys, &state.config, &token)
        .map(|claims| claims.jti)
        .or_else(|_| tokens::decode_id(&state.keys, &state.config, &token).map(|claims| claims.sid))
        .map_err(|_| AppError::unauthorized("invalid token"))?;

    let revoked = state.store.revoke_session(&session_id).await;
    Ok((no_store(), Json(json!({ "revoked": revoked }))).into_response())
}

// -------------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------------

fn no_store() -> [(header::HeaderName, &'static str); 2] {
    [
        (header::CACHE_CONTROL, "no-store"),
        (header::PRAGMA, "no-cache"),
    ]
}

fn normalize_scope(
    requested: Option<&str>,
    config: &Config,
    client: &ClientConfig,
) -> Result<String, AppError> {
    let requested = requested.unwrap_or("openid");
    let permitted = client.permitted_scopes(&config.allowed_scopes);
    let mut scopes = Vec::new();
    for scope in requested.split_whitespace() {
        if !permitted.iter().any(|allowed| allowed == scope) {
            return Err(AppError::new(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                format!("scope {scope} is not allowed"),
            ));
        }
        if !scopes.iter().any(|existing| existing == scope) {
            scopes.push(scope.to_owned());
        }
    }
    if !scopes.iter().any(|scope| scope == "openid") {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "invalid_scope",
            "the openid scope is required",
        ));
    }
    Ok(scopes.join(" "))
}

fn validate_pkce_challenge(challenge: &str, method: &str) -> Result<(), AppError> {
    if method != "S256" {
        return Err(AppError::invalid_request(
            "only code_challenge_method=S256 is supported",
        ));
    }
    if !valid_pkce_value(challenge) {
        return Err(AppError::invalid_request("invalid code_challenge"));
    }
    Ok(())
}

fn valid_pkce_value(value: &str) -> bool {
    (43..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    if !valid_pkce_value(verifier) {
        return false;
    }
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest) == challenge
}

fn missing_required_roles(client: &ClientConfig, roles: &[String]) -> Vec<String> {
    client
        .required_roles
        .iter()
        .filter(|required| !roles.iter().any(|held| held == *required))
        .cloned()
        .collect()
}

fn redirect_with_code(
    redirect_uri: &str,
    code: &str,
    state: Option<&str>,
) -> Result<String, AppError> {
    let mut url = Url::parse(redirect_uri)
        .map_err(|_| AppError::internal("invalid registered redirect_uri"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("code", code);
        if let Some(state) = state {
            query.append_pair("state", state);
        }
    }
    Ok(url.into())
}

fn bearer_token(headers: &HeaderMap) -> Result<String, AppError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("missing Authorization header"))?;
    let (scheme, token) = value
        .split_once(' ')
        .ok_or_else(|| AppError::unauthorized("malformed Authorization header"))?;
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return Err(AppError::unauthorized("expected a Bearer access token"));
    }
    Ok(token.to_owned())
}

fn has_basic_auth(headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("basic"))
}

fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = STANDARD.decode(encoded).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (client_id, client_secret) = decoded.split_once(':')?;
    Some((client_id.to_owned(), client_secret.to_owned()))
}

fn authenticate_client<'a>(
    config: &'a Config,
    headers: &HeaderMap,
    client_id: Option<&str>,
    client_secret: Option<&str>,
) -> Result<&'a ClientConfig, AppError> {
    let (client_id, client_secret) = match basic_credentials(headers) {
        Some((basic_id, basic_secret)) => {
            if let Some(form_id) = client_id {
                if form_id != basic_id {
                    return Err(AppError::invalid_client(
                        "client_id does not match the Basic authorization header",
                    ));
                }
            }
            (basic_id, Some(basic_secret))
        }
        None => (
            client_id
                .map(str::to_owned)
                .ok_or_else(|| AppError::invalid_client("missing client_id"))?,
            client_secret.map(str::to_owned),
        ),
    };

    let client = config
        .client(&client_id)
        .ok_or_else(|| AppError::invalid_client("unknown client"))?;
    if let Some(expected) = &client.client_secret {
        let provided = client_secret
            .ok_or_else(|| AppError::invalid_client("client authentication is required"))?;
        if !constant_time_eq(expected.as_bytes(), provided.as_bytes()) {
            return Err(AppError::invalid_client("invalid client credentials"));
        }
    }
    Ok(client)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.ct_eq(right).into()
}

async fn active_session(
    state: &AppState,
    session_id: &str,
    subject: &str,
) -> Result<Session, AppError> {
    let session = state
        .store
        .get_session(session_id)
        .await
        .ok_or_else(|| AppError::unauthorized("session no longer exists"))?;
    if session.revoked || session.account_id != subject || state.clock.now() >= session.expires_at {
        return Err(AppError::unauthorized("session is expired or revoked"));
    }
    Ok(session)
}

async fn ensure_live_roles(state: &AppState, session: &Session) -> Result<Vec<String>, AppError> {
    let roles = state
        .roles
        .roles_of(&session.account_id)
        .await
        .map_err(|err| AppError::internal(err.to_string()))?;
    let client = state
        .config
        .client(&session.client_id)
        .ok_or_else(|| AppError::internal("session references an unknown client"))?;
    let missing = missing_required_roles(client, &roles);
    if !missing.is_empty() {
        state.store.revoke_session(&session.id).await;
        return Err(AppError::unauthorized(format!(
            "required roles were revoked: {}",
            missing.join(", ")
        )));
    }
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> ClientConfig {
        ClientConfig {
            client_id: "app".to_owned(),
            name: None,
            redirect_uris: vec!["https://app.example.com/callback".to_owned()],
            client_secret: None,
            required_roles: vec!["admin".to_owned()],
            allowed_scopes: None,
        }
    }

    #[test]
    fn pkce_validation_accepts_s256_values() {
        assert!(valid_pkce_value(&"a".repeat(43)));
        assert!(valid_pkce_value(&"A-Za-z0-9._~".repeat(4)));
        assert!(!valid_pkce_value("short"));
        assert!(!valid_pkce_value(&"a".repeat(129)));
        assert!(!valid_pkce_value(
            "has space in the value has space in the value!"
        ));
    }

    #[test]
    fn pkce_verification_matches_the_s256_transform() {
        let verifier = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~";
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        assert!(verify_pkce(verifier, &challenge));
        assert!(!verify_pkce(verifier, "wrong"));
        assert!(!verify_pkce("short", &challenge));
    }

    #[test]
    fn scopes_must_be_allowed_and_include_openid() {
        let config = Config {
            issuer: "https://auth.example.com".to_owned(),
            listen: "127.0.0.1:0".to_owned(),
            rpc_url: "https://rpc.example.com".to_owned(),
            contract_id: "rbac.testnet".to_owned(),
            clients: vec![client()],
            nep413_recipient: None,
            nep413_callback_url: None,
            access_token_ttl_secs: 1,
            id_token_ttl_secs: 1,
            authorization_code_ttl_secs: 1,
            pending_login_ttl_secs: 1,
            max_pending_logins: 10_000,
            store_sweep_interval_secs: 60,
            signing_key_path: None,
            verify_access_key: true,
            allowed_scopes: vec!["openid".to_owned(), "profile".to_owned()],
        };
        assert_eq!(
            normalize_scope(Some("openid profile openid"), &config, &client()).unwrap(),
            "openid profile"
        );
        assert!(normalize_scope(Some("profile"), &config, &client()).is_err());
        assert!(normalize_scope(Some("openid email"), &config, &client()).is_err());
    }

    #[test]
    fn required_roles_are_all_required() {
        let client = client();
        assert!(missing_required_roles(&client, &["admin".to_owned()]).is_empty());
        assert_eq!(
            missing_required_roles(&client, &["employee".to_owned()]),
            vec!["admin".to_owned()]
        );
    }

    #[test]
    fn redirects_preserve_state() {
        let url =
            redirect_with_code("https://app.example.com/callback", "abc", Some("xyz")).unwrap();
        assert_eq!(url, "https://app.example.com/callback?code=abc&state=xyz");
    }

    #[test]
    fn decodes_basic_credentials() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            format!("Basic {}", STANDARD.encode("app:secret"))
                .parse()
                .unwrap(),
        );
        assert_eq!(
            basic_credentials(&headers),
            Some(("app".to_owned(), "secret".to_owned()))
        );
        assert!(has_basic_auth(&headers));
    }

    #[test]
    fn bearer_tokens_are_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "bearer abc".parse().unwrap());
        assert_eq!(bearer_token(&headers).unwrap(), "abc");
    }

    #[test]
    fn rejects_malformed_authorization_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert!(bearer_token(&headers).is_err());
    }
}
