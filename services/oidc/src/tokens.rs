//! JWT claim shapes and token helpers.

use thiserror::Error;

use crate::clock::Clock;
use crate::config::Config;
use crate::keys::KeyMaterial;
use crate::store::Session;

pub const ACCESS_TOKEN_USE: &str = "access";
pub const ID_TOKEN_USE: &str = "id";

/// Claims in an access token. `jti` is the session id.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccessClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub jti: String,
    pub client_id: String,
    pub scope: String,
    pub roles: Vec<String>,
    pub token_use: String,
}

/// Claims in an ID token.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct IdClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: u64,
    pub iat: u64,
    pub auth_time: u64,
    pub sid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    pub preferred_username: String,
    pub roles: Vec<String>,
    pub token_use: String,
}

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("invalid token: {0}")]
    Invalid(String),
    #[error("token has the wrong type")]
    WrongType,
}

pub fn access_claims(config: &Config, clock: &dyn Clock, session: &Session) -> AccessClaims {
    let now = clock.now();
    AccessClaims {
        iss: config.base_url().to_owned(),
        sub: session.account_id.clone(),
        aud: session.client_id.clone(),
        exp: now + config.access_token_ttl_secs,
        iat: now,
        jti: session.id.clone(),
        client_id: session.client_id.clone(),
        scope: session.scope.clone(),
        roles: session.roles.clone(),
        token_use: ACCESS_TOKEN_USE.to_owned(),
    }
}

pub fn id_claims(
    config: &Config,
    clock: &dyn Clock,
    session: &Session,
    nonce: Option<String>,
) -> IdClaims {
    let now = clock.now();
    IdClaims {
        iss: config.base_url().to_owned(),
        sub: session.account_id.clone(),
        aud: session.client_id.clone(),
        exp: now + config.id_token_ttl_secs,
        iat: now,
        auth_time: session.auth_time,
        sid: session.id.clone(),
        nonce,
        preferred_username: session.account_id.clone(),
        roles: session.roles.clone(),
        token_use: ID_TOKEN_USE.to_owned(),
    }
}

pub fn encode_access(
    keys: &KeyMaterial,
    config: &Config,
    clock: &dyn Clock,
    session: &Session,
) -> Result<String, TokenError> {
    keys.encode(&access_claims(config, clock, session))
        .map_err(|err| TokenError::Invalid(err.to_string()))
}

pub fn encode_id(
    keys: &KeyMaterial,
    config: &Config,
    clock: &dyn Clock,
    session: &Session,
    nonce: Option<String>,
) -> Result<String, TokenError> {
    keys.encode(&id_claims(config, clock, session, nonce))
        .map_err(|err| TokenError::Invalid(err.to_string()))
}

pub fn decode_access(
    keys: &KeyMaterial,
    config: &Config,
    token: &str,
) -> Result<AccessClaims, TokenError> {
    let claims: AccessClaims = keys
        .decode(token, config.base_url())
        .map_err(|err| TokenError::Invalid(err.to_string()))?;
    if claims.token_use != ACCESS_TOKEN_USE {
        return Err(TokenError::WrongType);
    }
    if claims.aud != claims.client_id {
        return Err(TokenError::Invalid(
            "access token audience does not match its client".to_owned(),
        ));
    }
    Ok(claims)
}

pub fn decode_id(keys: &KeyMaterial, config: &Config, token: &str) -> Result<IdClaims, TokenError> {
    let claims: IdClaims = keys
        .decode(token, config.base_url())
        .map_err(|err| TokenError::Invalid(err.to_string()))?;
    if claims.token_use != ID_TOKEN_USE {
        return Err(TokenError::WrongType);
    }
    Ok(claims)
}
