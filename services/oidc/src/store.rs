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

/// Default cap on pending logins when the store is constructed without an explicit limit.
pub const DEFAULT_MAX_PENDING_LOGINS: usize = 10_000;

/// Process-local store for pending logins, codes and sessions.
pub struct Store {
    pending: RwLock<HashMap<String, PendingLogin>>,
    codes: RwLock<HashMap<String, AuthorizationCode>>,
    sessions: RwLock<HashMap<String, Session>>,
    max_pending: usize,
}

impl Store {
    pub fn new() -> Self {
        Self::with_max_pending(DEFAULT_MAX_PENDING_LOGINS)
    }

    /// Creates a store that rejects new pending logins once `max_pending` are live.
    pub fn with_max_pending(max_pending: usize) -> Self {
        Self {
            pending: RwLock::new(HashMap::new()),
            codes: RwLock::new(HashMap::new()),
            sessions: RwLock::new(HashMap::new()),
            max_pending,
        }
    }

    /// Inserts a pending login and reports whether it was accepted.
    ///
    /// Expired entries are pruned first. Returns `false` when the store is at capacity, so the
    /// caller can reject the unauthenticated `/authorize` request instead of growing memory.
    pub async fn insert_pending(&self, pending: PendingLogin) -> bool {
        let mut entries = self.pending.write().await;
        let now = pending.created_at;
        entries.retain(|_, entry| entry.expires_at > now);
        if entries.len() >= self.max_pending {
            return false;
        }
        entries.insert(pending.id.clone(), pending);
        true
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

    /// Removes expired pending logins, authorization codes and sessions.
    ///
    /// Returns the number of removed entries. Call this periodically and on startup so abandoned
    /// unauthenticated requests cannot accumulate indefinitely.
    pub async fn prune_expired(&self, now: u64) -> usize {
        let mut removed = 0;

        let mut pending = self.pending.write().await;
        let before = pending.len();
        pending.retain(|_, entry| entry.expires_at > now);
        removed += before - pending.len();
        drop(pending);

        let mut codes = self.codes.write().await;
        let before = codes.len();
        codes.retain(|_, entry| entry.expires_at > now);
        removed += before - codes.len();
        drop(codes);

        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|_, entry| entry.expires_at > now);
        removed += before - sessions.len();

        removed
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(id: &str, created_at: u64, expires_at: u64) -> PendingLogin {
        PendingLogin {
            id: id.to_owned(),
            client_id: "app".to_owned(),
            redirect_uri: "https://app.example.com/callback".to_owned(),
            state: None,
            scope: "openid".to_owned(),
            oidc_nonce: None,
            code_challenge: "challenge".to_owned(),
            nep413_nonce: [0u8; 32],
            message: "sign in".to_owned(),
            created_at,
            expires_at,
        }
    }

    fn code(expires_at: u64) -> AuthorizationCode {
        AuthorizationCode {
            client_id: "app".to_owned(),
            redirect_uri: "https://app.example.com/callback".to_owned(),
            account_id: "alice.testnet".to_owned(),
            roles: vec!["admin".to_owned()],
            scope: "openid".to_owned(),
            oidc_nonce: None,
            code_challenge: "challenge".to_owned(),
            auth_time: 0,
            expires_at,
        }
    }

    fn session(id: &str, expires_at: u64) -> Session {
        Session {
            id: id.to_owned(),
            account_id: "alice.testnet".to_owned(),
            client_id: "app".to_owned(),
            roles: vec!["admin".to_owned()],
            scope: "openid".to_owned(),
            auth_time: 0,
            issued_at: 0,
            expires_at,
            revoked: false,
        }
    }

    #[tokio::test]
    async fn insert_pending_rejects_new_entries_at_capacity() {
        let store = Store::with_max_pending(2);
        assert!(store.insert_pending(pending("one", 0, 100)).await);
        assert!(store.insert_pending(pending("two", 0, 100)).await);
        assert!(!store.insert_pending(pending("three", 0, 100)).await);
        assert_eq!(store.pending_count().await, 2);
    }

    #[tokio::test]
    async fn insert_pending_prunes_expired_entries_before_capacity_check() {
        let store = Store::with_max_pending(1);
        assert!(store.insert_pending(pending("old", 0, 10)).await);
        assert!(store.insert_pending(pending("new", 20, 100)).await);
        assert_eq!(store.pending_count().await, 1);
        assert!(store.get_pending("old").await.is_none());
    }

    #[tokio::test]
    async fn prune_expired_removes_all_expired_entry_types() {
        let store = Store::new();
        assert!(store.insert_pending(pending("live", 0, 100)).await);
        assert!(store.insert_pending(pending("expired", 0, 10)).await);
        store.insert_code("code".to_owned(), code(10)).await;
        store.insert_session(session("session", 10)).await;

        assert_eq!(store.prune_expired(20).await, 3);
        assert_eq!(store.pending_count().await, 1);
        assert_eq!(store.code_count().await, 0);
        assert_eq!(store.session_count().await, 0);
    }
}
