//! In-memory authorization requests, authorization codes and sessions.
//!
//! This implementation is intentionally process-local. A single instance is fine for a
//! hackathon deployment; production deployments should replace this store with a shared,
//! persistent database when running more than one replica.

use std::collections::HashMap;

use rand::rngs::OsRng;
use rand::RngCore;
use tokio::sync::RwLock;

/// A login page that has been created but not completed yet.
#[derive(Clone, Debug)]
pub struct PendingLogin {
    pub id: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub scope: String,
    pub oidc_nonce: Option<String>,
    pub code_challenge: String,
    pub nep413_nonce: [u8; 32],
    pub message: String,
    pub created_at: u64,
    pub expires_at: u64,
}

/// A one-time authorization code bound to a successful NEP-413 login.
#[derive(Clone, Debug)]
pub struct AuthorizationCode {
    pub client_id: String,
    pub redirect_uri: String,
    pub account_id: String,
    pub roles: Vec<String>,
    pub scope: String,
    pub oidc_nonce: Option<String>,
    pub code_challenge: String,
    pub auth_time: u64,
    pub expires_at: u64,
}

/// An issued login session. Access tokens reference the session by id.
#[derive(Clone, Debug)]
pub struct Session {
    pub id: String,
    pub account_id: String,
    pub client_id: String,
    pub roles: Vec<String>,
    pub scope: String,
    pub auth_time: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub revoked: bool,
}

/// Process-local store for pending logins, codes and sessions.
#[derive(Default)]
pub struct Store {
    pending: RwLock<HashMap<String, PendingLogin>>,
    codes: RwLock<HashMap<String, AuthorizationCode>>,
    sessions: RwLock<HashMap<String, Session>>,
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn insert_pending(&self, pending: PendingLogin) {
        self.pending
            .write()
            .await
            .insert(pending.id.clone(), pending);
    }

    pub async fn get_pending(&self, id: &str) -> Option<PendingLogin> {
        self.pending.read().await.get(id).cloned()
    }

    pub async fn remove_pending(&self, id: &str) -> Option<PendingLogin> {
        self.pending.write().await.remove(id)
    }

    pub async fn insert_code(&self, code: String, value: AuthorizationCode) {
        self.codes.write().await.insert(code, value);
    }

    /// Removes and returns a code. Codes are single-use even when the exchange fails later.
    pub async fn take_code(&self, code: &str) -> Option<AuthorizationCode> {
        self.codes.write().await.remove(code)
    }

    pub async fn insert_session(&self, session: Session) {
        self.sessions
            .write()
            .await
            .insert(session.id.clone(), session);
    }

    pub async fn get_session(&self, id: &str) -> Option<Session> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn revoke_session(&self, id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.revoked = true;
            true
        } else {
            false
        }
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    pub async fn pending_count(&self) -> usize {
        self.pending.read().await.len()
    }

    pub async fn code_count(&self) -> usize {
        self.codes.read().await.len()
    }
}

/// Generates a URL-safe, unguessable 256-bit token.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Generates a 256-bit NEP-413 nonce.
pub fn random_nonce() -> [u8; 32] {
    let mut nonce = [0u8; 32];
    OsRng.fill_bytes(&mut nonce);
    nonce
}
