//! End-to-end HTTP tests for the OIDC provider.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, Request, Response, StatusCode};
use axum::Router;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use http_body_util::BodyExt;
use payrole_oidc::clock::Clock;
use payrole_oidc::config::{ClientConfig, Config};
use payrole_oidc::keys::KeyMaterial;
use payrole_oidc::roles::{RoleSource, RoleSourceError};
use payrole_oidc::tokens::{self, AccessClaims, IdClaims};
use payrole_oidc::{build_router, AppState};
use rand::rngs::OsRng;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;
use url::form_urlencoded;
use url::Url;

const ISSUER: &str = "https://auth.example.com";
const RECIPIENT: &str = "auth.example.com";
const CONTRACT: &str = "payrole.testnet";
const APP: &str = "demo-app";
const APP_REDIRECT: &str = "https://app.example.com/callback";
const SERVICE: &str = "service-app";
const SERVICE_SECRET: &str = "s3cret";
const SERVICE_REDIRECT: &str = "https://service.example.com/callback";
const ALICE: &str = "alice.test.near";
const VERIFIER: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~";

#[derive(Clone)]
struct FakeClock {
    now: Arc<AtomicU64>,
}

impl FakeClock {
    fn new(now: u64) -> Self {
        Self {
            now: Arc::new(AtomicU64::new(now)),
        }
    }

    fn advance(&self, seconds: u64) {
        self.now.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> u64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Default)]
struct StaticRoles {
    roles: Arc<RwLock<HashMap<String, Vec<String>>>>,
    keys: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl StaticRoles {
    fn set_roles(&self, account: &str, roles: &[&str]) {
        self.roles.write().unwrap().insert(
            account.to_owned(),
            roles.iter().map(|role| (*role).to_owned()).collect(),
        );
    }

    fn clear_roles(&self, account: &str) {
        self.roles.write().unwrap().remove(account);
    }

    fn add_key(&self, account: &str, public_key: &str) {
        self.keys
            .write()
            .unwrap()
            .entry(account.to_owned())
            .or_default()
            .push(public_key.to_owned());
    }
}

#[async_trait]
impl RoleSource for StaticRoles {
    async fn roles_of(&self, account_id: &str) -> Result<Vec<String>, RoleSourceError> {
        Ok(self
            .roles
            .read()
            .unwrap()
            .get(account_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn has_access_key(
        &self,
        account_id: &str,
        public_key: &str,
    ) -> Result<bool, RoleSourceError> {
        Ok(self
            .keys
            .read()
            .unwrap()
            .get(account_id)
            .is_some_and(|keys| keys.iter().any(|key| key == public_key)))
    }
}

struct TestApp {
    router: Router,
    state: AppState,
    clock: FakeClock,
    roles: StaticRoles,
    signing_key: SigningKey,
}

impl TestApp {
    fn new() -> Self {
        Self::with_max_pending(10_000)
    }

    fn with_max_pending(max_pending_logins: usize) -> Self {
        let config = Config {
            issuer: ISSUER.to_owned(),
            listen: "127.0.0.1:0".to_owned(),
            rpc_url: "https://rpc.example.com".to_owned(),
            contract_id: CONTRACT.to_owned(),
            clients: vec![
                ClientConfig {
                    client_id: APP.to_owned(),
                    name: Some("Demo app".to_owned()),
                    redirect_uris: vec![APP_REDIRECT.to_owned()],
                    client_secret: None,
                    required_roles: vec!["admin".to_owned()],
                    allowed_scopes: None,
                },
                ClientConfig {
                    client_id: SERVICE.to_owned(),
                    name: Some("Service".to_owned()),
                    redirect_uris: vec![SERVICE_REDIRECT.to_owned()],
                    client_secret: Some(SERVICE_SECRET.to_owned()),
                    required_roles: Vec::new(),
                    allowed_scopes: None,
                },
            ],
            nep413_recipient: None,
            nep413_callback_url: None,
            access_token_ttl_secs: 900,
            id_token_ttl_secs: 900,
            authorization_code_ttl_secs: 300,
            pending_login_ttl_secs: 300,
            max_pending_logins,
            store_sweep_interval_secs: 60,
            signing_key_path: None,
            verify_access_key: true,
            allowed_scopes: vec![
                "openid".to_owned(),
                "profile".to_owned(),
                "roles".to_owned(),
            ],
        };
        config.validate().unwrap();

        // JWT expiry is validated against the real system clock by `jsonwebtoken`, so the
        // fake clock must start at the current time. Individual tests can still advance it.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let clock = FakeClock::new(now);
        let roles = StaticRoles::default();
        let signing_key = SigningKey::generate(&mut OsRng);
        roles.add_key(ALICE, &public_key(&signing_key));

        let state = AppState::new(
            config,
            test_keys(),
            Arc::new(roles.clone()),
            Arc::new(clock.clone()),
        );
        let router = build_router(state.clone());

        Self {
            router,
            state,
            clock,
            roles,
            signing_key,
        }
    }

    fn sign(&self, message: &str, nonce: [u8; 32]) -> String {
        sign_with(&self.signing_key, message, nonce)
    }

    fn public_key(&self) -> String {
        public_key(&self.signing_key)
    }

    async fn send(&self, request: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(request).await.unwrap()
    }
}

fn test_keys() -> KeyMaterial {
    static KEYS: OnceLock<KeyMaterial> = OnceLock::new();
    KEYS.get_or_init(|| KeyMaterial::generate().unwrap())
        .clone()
}

fn public_key(key: &SigningKey) -> String {
    format!(
        "ed25519:{}",
        bs58::encode(key.verifying_key().to_bytes()).into_string()
    )
}

fn sign_with(key: &SigningKey, message: &str, nonce: [u8; 32]) -> String {
    let digest = payrole_oidc::nep413::signing_hash(message, nonce, RECIPIENT, None);
    STANDARD.encode(key.sign(&digest).to_bytes())
}

fn challenge() -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()))
}

fn encode_pairs(pairs: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(pairs.iter().copied());
    serializer.finish()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn form_post(uri: &str, pairs: &[(&str, &str)]) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(encode_pairs(pairs)))
        .unwrap()
}

fn bearer_get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_text(response: Response<Body>) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn body_json(response: Response<Body>) -> (StatusCode, Value) {
    let status = response.status();
    let text = body_text(response).await;
    let json = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, json)
}

fn extract_between(haystack: &str, start: &str, end: &str) -> String {
    let start = haystack
        .find(start)
        .unwrap_or_else(|| panic!("missing marker {start:?}"))
        + start.len();
    let rest = &haystack[start..];
    let end = rest
        .find(end)
        .unwrap_or_else(|| panic!("missing end marker {end:?}"));
    rest[..end].to_owned()
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

struct Pending {
    request_id: String,
    message: String,
    nonce: [u8; 32],
}

async fn start_authorize(
    app: &TestApp,
    client_id: &str,
    redirect_uri: &str,
    scope: &str,
    oidc_nonce: Option<&str>,
) -> Pending {
    let challenge = challenge();
    let mut pairs = vec![
        ("response_type", "code"),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("scope", scope),
        ("state", "state-123"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ];
    if let Some(nonce) = oidc_nonce {
        pairs.push(("nonce", nonce));
    }
    let uri = format!("/authorize?{}", encode_pairs(&pairs));
    let response = app.send(get(&uri)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let html = body_text(response).await;
    assert!(html.contains("Demo app") || html.contains("Service"));

    let request_id = extract_between(&html, "name=\"request_id\" value=\"", "\"");
    let message = html_unescape(&extract_between(
        &html,
        "id=\"message\" readonly>",
        "</textarea>",
    ));
    let nonce = extract_between(&html, "Nonce (base64): <code>", "</code>");
    let nonce: [u8; 32] = STANDARD.decode(nonce).unwrap().try_into().unwrap();

    Pending {
        request_id,
        message,
        nonce,
    }
}

async fn login(
    app: &TestApp,
    pending: &Pending,
    account_id: &str,
    public_key: &str,
    signature: &str,
) -> Response<Body> {
    app.send(form_post(
        "/login",
        &[
            ("request_id", pending.request_id.as_str()),
            ("account_id", account_id),
            ("public_key", public_key),
            ("signature", signature),
        ],
    ))
    .await
}

fn code_from_redirect(response: &Response<Body>) -> String {
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("Location header")
        .to_str()
        .unwrap();
    let url = Url::parse(location).unwrap();
    let params: HashMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    assert_eq!(params.get("state").map(String::as_str), Some("state-123"));
    params.get("code").expect("code").to_owned()
}

async fn authorize_and_login(
    app: &TestApp,
    client_id: &str,
    redirect_uri: &str,
    account_id: &str,
    oidc_nonce: Option<&str>,
) -> String {
    let pending = start_authorize(
        app,
        client_id,
        redirect_uri,
        "openid profile roles",
        oidc_nonce,
    )
    .await;
    let signature = app.sign(&pending.message, pending.nonce);
    let response = login(app, &pending, account_id, &app.public_key(), &signature).await;
    code_from_redirect(&response)
}

fn token_request(
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: Option<&str>,
) -> Request<Body> {
    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", VERIFIER),
    ];
    if let Some(secret) = client_secret {
        pairs.push(("client_secret", secret));
    }
    form_post("/token", &pairs)
}

fn basic_token_request(
    code: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: &str,
) -> Request<Body> {
    let mut request = form_post(
        "/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", VERIFIER),
        ],
    );
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!(
            "Basic {}",
            STANDARD.encode(format!("{client_id}:{client_secret}"))
        )
        .parse()
        .unwrap(),
    );
    request
}

fn introspect_request(token: &str) -> Request<Body> {
    form_post("/introspect", &[("token", token)])
}

fn basic_introspect_request(token: &str, client_id: &str, client_secret: &str) -> Request<Body> {
    let mut request = introspect_request(token);
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!(
            "Basic {}",
            STANDARD.encode(format!("{client_id}:{client_secret}"))
        )
        .parse()
        .unwrap(),
    );
    request
}

#[tokio::test]
async fn discovery_and_jwks_describe_the_provider() {
    let app = TestApp::new();

    let response = app.send(get("/.well-known/openid-configuration")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, discovery) = body_json(response).await;
    assert_eq!(discovery["issuer"], ISSUER);
    assert_eq!(
        discovery["authorization_endpoint"],
        format!("{ISSUER}/authorize")
    );
    assert_eq!(discovery["token_endpoint"], format!("{ISSUER}/token"));
    assert_eq!(discovery["jwks_uri"], format!("{ISSUER}/jwks.json"));
    assert_eq!(
        discovery["introspection_endpoint"],
        format!("{ISSUER}/introspect")
    );
    assert_eq!(discovery["response_types_supported"][0], "code");
    assert_eq!(discovery["code_challenge_methods_supported"][0], "S256");
    assert_eq!(
        discovery["token_endpoint_auth_methods_supported"][2],
        "client_secret_basic"
    );

    let response = app.send(get("/jwks.json")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, jwks) = body_json(response).await;
    assert_eq!(jwks["keys"][0]["kty"], "RSA");
    assert_eq!(jwks["keys"][0]["alg"], "RS256");
    assert_eq!(jwks["keys"][0]["use"], "sig");
    assert_eq!(jwks["keys"][0]["kid"], app.state.keys.kid());
    assert!(!jwks["keys"][0]["n"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn complete_authorization_code_flow() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin", "employee"]);

    let pending = start_authorize(
        &app,
        APP,
        APP_REDIRECT,
        "openid profile roles",
        Some("oidc-nonce"),
    )
    .await;
    assert!(pending.message.contains("Demo app"));
    assert!(pending.message.contains(ISSUER));
    assert!(pending.message.contains(APP));

    let signature = app.sign(&pending.message, pending.nonce);
    let response = login(&app, &pending, ALICE, &app.public_key(), &signature).await;
    let code = code_from_redirect(&response);

    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, token) = body_json(response).await;
    assert_eq!(token["token_type"], "Bearer");
    assert_eq!(token["expires_in"], 900);
    assert_eq!(token["scope"], "openid profile roles");
    let access_token = token["access_token"].as_str().unwrap();
    let id_token = token["id_token"].as_str().unwrap();

    let access: AccessClaims =
        tokens::decode_access(&app.state.keys, &app.state.config, access_token).unwrap();
    assert_eq!(access.sub, ALICE);
    assert_eq!(access.aud, APP);
    assert_eq!(access.client_id, APP);
    assert_eq!(
        access.roles,
        vec!["admin".to_owned(), "employee".to_owned()]
    );
    assert_eq!(access.token_use, "access");

    let id: IdClaims = tokens::decode_id(&app.state.keys, &app.state.config, id_token).unwrap();
    assert_eq!(id.sub, ALICE);
    assert_eq!(id.aud, APP);
    assert_eq!(id.nonce.as_deref(), Some("oidc-nonce"));
    assert_eq!(id.preferred_username, ALICE);
    assert_eq!(id.roles, access.roles);

    let response = app.send(bearer_get("/userinfo", access_token)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, userinfo) = body_json(response).await;
    assert_eq!(userinfo["sub"], ALICE);
    assert_eq!(userinfo["preferred_username"], ALICE);
    assert_eq!(userinfo["roles"][0], "admin");

    let response = app.send(introspect_request(access_token)).await;
    let (status, introspection) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(introspection["active"], true);
    assert_eq!(introspection["sub"], ALICE);
    assert_eq!(introspection["client_id"], APP);
    assert_eq!(introspection["roles"][1], "employee");

    let response = app
        .send(form_post("/logout", &[("token", access_token)]))
        .await;
    let (_, logout) = body_json(response).await;
    assert_eq!(logout["revoked"], true);

    let response = app.send(introspect_request(access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], false);

    let response = app.send(bearer_get("/userinfo", access_token)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_accepts_an_id_token_hint() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;

    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (_, token) = body_json(response).await;
    let access_token = token["access_token"].as_str().unwrap().to_owned();
    let id_token = token["id_token"].as_str().unwrap().to_owned();

    let response = app
        .send(form_post(
            "/logout",
            &[("id_token_hint", id_token.as_str())],
        ))
        .await;
    let (_, logout) = body_json(response).await;
    assert_eq!(logout["revoked"], true);

    let response = app.send(introspect_request(&access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], false);
}

#[tokio::test]
async fn live_role_revocation_invalidates_access_tokens() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (_, token) = body_json(response).await;
    let access_token = token["access_token"].as_str().unwrap().to_owned();

    let response = app.send(introspect_request(&access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], true);

    app.roles.clear_roles(ALICE);

    let response = app.send(introspect_request(&access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], false);

    let response = app.send(bearer_get("/userinfo", &access_token)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // The failed live check revokes the session, so re-granting the role does not resurrect it.
    app.roles.set_roles(ALICE, &["admin"]);
    let response = app.send(introspect_request(&access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], false);
}

#[tokio::test]
async fn login_requires_a_valid_signature_and_a_registered_access_key() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);
    let pending = start_authorize(&app, APP, APP_REDIRECT, "openid", None).await;

    // Correct signature, but the key is not registered as an access key of the account.
    let other_key = SigningKey::generate(&mut OsRng);
    let signature = sign_with(&other_key, &pending.message, pending.nonce);
    let response = login(&app, &pending, ALICE, &public_key(&other_key), &signature).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_signature");
    assert!(error["error_description"]
        .as_str()
        .unwrap()
        .contains("access key"));

    // Registered key, but a signature over a different message.
    let wrong_signature = app.sign("some other message", pending.nonce);
    let response = login(&app, &pending, ALICE, &app.public_key(), &wrong_signature).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_signature");

    // Correct signature and registered key.
    let signature = app.sign(&pending.message, pending.nonce);
    let response = login(&app, &pending, ALICE, &app.public_key(), &signature).await;
    code_from_redirect(&response);
}

#[tokio::test]
async fn login_requires_the_clients_roles() {
    let app = TestApp::new();
    let pending = start_authorize(&app, APP, APP_REDIRECT, "openid", None).await;
    let signature = app.sign(&pending.message, pending.nonce);
    let response = login(&app, &pending, ALICE, &app.public_key(), &signature).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error["error"], "access_denied");
    assert!(error["error_description"]
        .as_str()
        .unwrap()
        .contains("admin"));

    app.roles.set_roles(ALICE, &["admin"]);
    let response = login(&app, &pending, ALICE, &app.public_key(), &signature).await;
    code_from_redirect(&response);
}

#[tokio::test]
async fn authorize_rejects_invalid_requests() {
    let app = TestApp::new();

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "code"),
                ("client_id", "unknown"),
                ("redirect_uri", APP_REDIRECT),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "S256"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_request");

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "code"),
                ("client_id", APP),
                ("redirect_uri", "https://evil.example.com/callback"),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "S256"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_request");

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "token"),
                ("client_id", APP),
                ("redirect_uri", APP_REDIRECT),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "S256"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "unsupported_response_type");

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "code"),
                ("client_id", APP),
                ("redirect_uri", APP_REDIRECT),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "plain"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_request");

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "code"),
                ("client_id", APP),
                ("redirect_uri", APP_REDIRECT),
                ("scope", "openid email"),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "S256"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_scope");

    let response = app
        .send(get(&format!(
            "/authorize?{}",
            encode_pairs(&[
                ("response_type", "code"),
                ("client_id", APP),
                ("redirect_uri", APP_REDIRECT),
                ("scope", "profile"),
                ("code_challenge", &challenge()),
                ("code_challenge_method", "S256"),
            ])
        )))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_scope");
}

#[tokio::test]
async fn token_rejects_pkce_mismatch_and_code_reuse() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;

    let request = form_post(
        "/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", APP_REDIRECT),
            ("client_id", APP),
            (
                "code_verifier",
                "wrong-but-long-enough-verifier-abcdefghijklmnopqrstuvwxyz",
            ),
        ],
    );
    let response = app.send(request).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");

    // The code is consumed even when the exchange fails.
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");

    // A fresh code can be exchanged exactly once.
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
}

#[tokio::test]
async fn authorization_codes_are_bound_to_client_and_redirect_uri() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);

    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    let request = form_post(
        "/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", "https://app.example.com/other"),
            ("client_id", APP),
            ("code_verifier", VERIFIER),
        ],
    );
    let response = app.send(request).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");

    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    let response = app
        .send(basic_token_request(
            &code,
            APP_REDIRECT,
            SERVICE,
            SERVICE_SECRET,
        ))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
}

#[tokio::test]
async fn confidential_clients_use_basic_authentication() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);
    let code = authorize_and_login(&app, SERVICE, SERVICE_REDIRECT, ALICE, None).await;

    let response = app
        .send(basic_token_request(
            &code,
            SERVICE_REDIRECT,
            SERVICE,
            SERVICE_SECRET,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let code = authorize_and_login(&app, SERVICE, SERVICE_REDIRECT, ALICE, None).await;
    let response = app
        .send(basic_token_request(
            &code,
            SERVICE_REDIRECT,
            SERVICE,
            "wrong",
        ))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_client");

    // client_secret_post is also supported.
    let code = authorize_and_login(&app, SERVICE, SERVICE_REDIRECT, ALICE, None).await;
    let response = app
        .send(token_request(
            &code,
            SERVICE_REDIRECT,
            SERVICE,
            Some(SERVICE_SECRET),
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn codes_logins_and_tokens_expire() {
    let app = TestApp::new();
    app.roles.set_roles(ALICE, &["admin"]);

    // Pending login expiry.
    let pending = start_authorize(&app, APP, APP_REDIRECT, "openid", None).await;
    app.clock.advance(301);
    let signature = app.sign(&pending.message, pending.nonce);
    let response = login(&app, &pending, ALICE, &app.public_key(), &signature).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_request");

    // Authorization code expiry.
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    app.clock.advance(301);
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");

    // Access token expiry.
    let code = authorize_and_login(&app, APP, APP_REDIRECT, ALICE, None).await;
    let response = app
        .send(token_request(&code, APP_REDIRECT, APP, None))
        .await;
    let (_, token) = body_json(response).await;
    let access_token = token["access_token"].as_str().unwrap().to_owned();
    app.clock.advance(901);

    let response = app.send(introspect_request(&access_token)).await;
    let (_, introspection) = body_json(response).await;
    assert_eq!(introspection["active"], false);
    let response = app.send(bearer_get("/userinfo", &access_token)).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn introspection_requires_matching_client_credentials_for_confidential_clients() {
    let app = TestApp::new();
    let code = authorize_and_login(&app, SERVICE, SERVICE_REDIRECT, ALICE, None).await;
    let response = app
        .send(basic_token_request(
            &code,
            SERVICE_REDIRECT,
            SERVICE,
            SERVICE_SECRET,
        ))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, token) = body_json(response).await;
    let access_token = token["access_token"].as_str().unwrap().to_owned();

    // A confidential client's token cannot be introspected anonymously.
    let response = app.send(introspect_request(&access_token)).await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_client");

    // Wrong credentials are rejected.
    let response = app
        .send(basic_introspect_request(&access_token, SERVICE, "wrong"))
        .await;
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_client");

    // A different client cannot see another client's token.
    let response = app
        .send(basic_introspect_request(&access_token, APP, "unused"))
        .await;
    let (status, introspection) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(introspection["active"], false);

    // The owning client can introspect it.
    let response = app
        .send(basic_introspect_request(
            &access_token,
            SERVICE,
            SERVICE_SECRET,
        ))
        .await;
    let (status, introspection) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(introspection["active"], true);
    assert_eq!(introspection["client_id"], SERVICE);
}

#[tokio::test]
async fn unknown_tokens_are_inactive() {
    let app = TestApp::new();
    let response = app.send(introspect_request("not-a-token")).await;
    let (status, introspection) = body_json(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(introspection["active"], false);
}

#[tokio::test]
async fn userinfo_requires_a_bearer_token() {
    let app = TestApp::new();
    let response = app.send(get("/userinfo")).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let (status, error) = body_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_token");
}

#[tokio::test]
async fn authorize_returns_429_when_pending_logins_are_full() {
    let app = TestApp::with_max_pending(1);
    let challenge = challenge();
    let uri = format!(
        "/authorize?{}",
        encode_pairs(&[
            ("response_type", "code"),
            ("client_id", APP),
            ("redirect_uri", APP_REDIRECT),
            ("scope", "openid profile roles"),
            ("state", "state-123"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ])
    );

    let first = app.send(get(&uri)).await;
    assert_eq!(first.status(), StatusCode::OK);

    let second = app.send(get(&uri)).await;
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    let (status, body) = body_json(second).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "temporarily_unavailable");
}
