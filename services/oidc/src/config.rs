//! Configuration for the OIDC provider.
//!
//! Configuration is read from a TOML file. Client secrets are optional: clients without a secret
//! are public clients and must use PKCE.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::Deserialize;
use url::Url;

fn default_listen() -> String {
    "127.0.0.1:8080".to_owned()
}

fn default_access_token_ttl_secs() -> u64 {
    900
}

fn default_id_token_ttl_secs() -> u64 {
    900
}

fn default_authorization_code_ttl_secs() -> u64 {
    300
}

fn default_pending_login_ttl_secs() -> u64 {
    300
}

fn default_true() -> bool {
    true
}

fn default_allowed_scopes() -> Vec<String> {
    vec![
        "openid".to_owned(),
        "profile".to_owned(),
        "roles".to_owned(),
    ]
}

/// One registered OIDC client.
#[derive(Clone, Debug, Deserialize)]
pub struct ClientConfig {
    /// Stable client identifier used by the application.
    pub client_id: String,
    /// Human-readable name shown on the login page.
    #[serde(default)]
    pub name: Option<String>,
    /// Exact redirect URIs registered for this client.
    pub redirect_uris: Vec<String>,
    /// Optional secret. When set, the client is confidential and must authenticate at the token
    /// endpoint. When omitted, the client is public and PKCE is required.
    #[serde(default)]
    pub client_secret: Option<String>,
    /// Roles the NEAR account must hold on-chain before an authorization code is issued.
    #[serde(default)]
    pub required_roles: Vec<String>,
    /// Optional per-client scope restriction. Falls back to `allowed_scopes`.
    #[serde(default)]
    pub allowed_scopes: Option<Vec<String>>,
}

impl ClientConfig {
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.client_id)
    }

    pub fn permitted_scopes<'a>(&'a self, global: &'a [String]) -> &'a [String] {
        self.allowed_scopes.as_deref().unwrap_or(global)
    }
}

/// Provider configuration.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// Public issuer URL. It is used as the `iss` claim and as the base of discovery URLs.
    pub issuer: String,
    /// Address to listen on.
    #[serde(default = "default_listen")]
    pub listen: String,
    /// NEAR JSON-RPC endpoint used to query role memberships.
    pub rpc_url: String,
    /// Account id of the RBAC contract.
    pub contract_id: String,
    /// Registered clients.
    pub clients: Vec<ClientConfig>,

    /// NEP-413 recipient. Defaults to the host part of `issuer`.
    #[serde(default)]
    pub nep413_recipient: Option<String>,
    /// Optional NEP-413 callback URL. Wallets include it in the signed payload.
    #[serde(default)]
    pub nep413_callback_url: Option<String>,

    /// Lifetime of access tokens and sessions, in seconds.
    #[serde(default = "default_access_token_ttl_secs")]
    pub access_token_ttl_secs: u64,
    /// Lifetime of ID tokens, in seconds.
    #[serde(default = "default_id_token_ttl_secs")]
    pub id_token_ttl_secs: u64,
    /// Lifetime of a one-time authorization code, in seconds.
    #[serde(default = "default_authorization_code_ttl_secs")]
    pub authorization_code_ttl_secs: u64,
    /// How long a login page / NEP-413 challenge stays valid, in seconds.
    #[serde(default = "default_pending_login_ttl_secs")]
    pub pending_login_ttl_secs: u64,

    /// Optional PKCS#8 PEM RSA private key. If omitted, an ephemeral key is generated at startup.
    #[serde(default)]
    pub signing_key_path: Option<PathBuf>,

    /// Whether to check that the NEP-413 public key is an access key of the claimed account.
    /// Keep this enabled outside local experiments.
    #[serde(default = "default_true")]
    pub verify_access_key: bool,

    /// Scopes accepted by the provider.
    #[serde(default = "default_allowed_scopes")]
    pub allowed_scopes: Vec<String>,
}

impl Config {
    /// Loads and validates a TOML configuration file.
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let config: Self = toml::from_str(&text)
            .with_context(|| format!("failed to parse config file {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Validates cross-field invariants.
    pub fn validate(&self) -> anyhow::Result<()> {
        let issuer = Url::parse(&self.issuer).context("issuer must be an absolute URL")?;
        if issuer.cannot_be_a_base() {
            bail!("issuer must be a hierarchical URL");
        }
        if self.issuer.ends_with('/') {
            bail!("issuer must not end with a slash");
        }
        if self.clients.is_empty() {
            bail!("at least one client must be configured");
        }
        if self.access_token_ttl_secs == 0 {
            bail!("access_token_ttl_secs must be positive");
        }
        if self.id_token_ttl_secs == 0 {
            bail!("id_token_ttl_secs must be positive");
        }
        if self.authorization_code_ttl_secs == 0 {
            bail!("authorization_code_ttl_secs must be positive");
        }
        if self.pending_login_ttl_secs == 0 {
            bail!("pending_login_ttl_secs must be positive");
        }
        if !self.allowed_scopes.iter().any(|scope| scope == "openid") {
            bail!("allowed_scopes must contain the openid scope");
        }
        if self
            .allowed_scopes
            .iter()
            .any(|scope| scope.is_empty() || scope.chars().any(char::is_whitespace))
        {
            bail!("allowed_scopes entries must be non-empty and contain no whitespace");
        }

        let mut ids = std::collections::HashSet::new();
        for client in &self.clients {
            if client.client_id.is_empty() {
                bail!("client_id must not be empty");
            }
            if !ids.insert(client.client_id.as_str()) {
                bail!("duplicate client_id {}", client.client_id);
            }
            if client.redirect_uris.is_empty() {
                bail!("client {} has no redirect_uris", client.client_id);
            }
            for redirect_uri in &client.redirect_uris {
                Url::parse(redirect_uri).with_context(|| {
                    format!(
                        "client {} has an invalid redirect_uri {}",
                        client.client_id, redirect_uri
                    )
                })?;
            }
            if client.client_secret.as_deref() == Some("") {
                bail!("client {} has an empty client_secret", client.client_id);
            }
            if let Some(scopes) = &client.allowed_scopes {
                if !scopes.iter().any(|scope| scope == "openid") {
                    bail!(
                        "client {} allowed_scopes must contain the openid scope",
                        client.client_id
                    );
                }
            }
        }
        Ok(())
    }

    pub fn client(&self, client_id: &str) -> Option<&ClientConfig> {
        self.clients
            .iter()
            .find(|client| client.client_id == client_id)
    }

    /// Issuer without a trailing slash.
    pub fn base_url(&self) -> &str {
        self.issuer.trim_end_matches('/')
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.base_url(), path)
    }

    /// Recipient string included in the NEP-413 payload.
    pub fn recipient(&self) -> String {
        if let Some(recipient) = &self.nep413_recipient {
            return recipient.clone();
        }
        Url::parse(&self.issuer)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| self.issuer.clone())
    }

    pub fn nep413_callback_url(&self) -> Option<&str> {
        self.nep413_callback_url.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> Config {
        Config {
            issuer: "https://auth.example.com".to_owned(),
            listen: "127.0.0.1:8080".to_owned(),
            rpc_url: "https://rpc.testnet.near.org".to_owned(),
            contract_id: "rbac.testnet".to_owned(),
            clients: vec![ClientConfig {
                client_id: "app".to_owned(),
                name: None,
                redirect_uris: vec!["https://app.example.com/callback".to_owned()],
                client_secret: None,
                required_roles: vec!["admin".to_owned()],
                allowed_scopes: None,
            }],
            nep413_recipient: None,
            nep413_callback_url: None,
            access_token_ttl_secs: 900,
            id_token_ttl_secs: 900,
            authorization_code_ttl_secs: 300,
            pending_login_ttl_secs: 300,
            signing_key_path: None,
            verify_access_key: true,
            allowed_scopes: default_allowed_scopes(),
        }
    }

    #[test]
    fn valid_config_passes() {
        base_config().validate().unwrap();
    }

    #[test]
    fn example_config_is_valid() {
        let text = include_str!("../config.example.toml");
        let config: Config = toml::from_str(text).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn recipient_defaults_to_issuer_host() {
        assert_eq!(base_config().recipient(), "auth.example.com");
    }

    #[test]
    fn explicit_recipient_wins() {
        let mut config = base_config();
        config.nep413_recipient = Some("wallet.example.com".to_owned());
        assert_eq!(config.recipient(), "wallet.example.com");
    }

    #[test]
    fn rejects_trailing_slash_in_issuer() {
        let mut config = base_config();
        config.issuer = "https://auth.example.com/".to_owned();
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_clients() {
        let mut config = base_config();
        config.clients.push(config.clients[0].clone());
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_missing_openid_scope() {
        let mut config = base_config();
        config.allowed_scopes = vec!["profile".to_owned()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn endpoints_are_derived_from_the_issuer() {
        let config = base_config();
        assert_eq!(config.endpoint("/token"), "https://auth.example.com/token");
    }
}
