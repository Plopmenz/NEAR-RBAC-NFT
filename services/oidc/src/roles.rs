//! Sources of on-chain identity and role membership.

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use thiserror::Error;

/// Errors returned by an identity/role source.
#[derive(Debug, Error)]
pub enum RoleSourceError {
    #[error("NEAR RPC request failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("NEAR RPC returned an error: {0}")]
    Rpc(String),
    #[error("NEAR RPC returned an unexpected response: {0}")]
    Unexpected(String),
    #[error("failed to decode the contract result: {0}")]
    Decode(String),
}

/// Provides role membership and access-key information for a NEAR account.
#[async_trait]
pub trait RoleSource: Send + Sync {
    /// Returns the roles currently held by `account_id`, as reported by the RBAC contract.
    async fn roles_of(&self, account_id: &str) -> Result<Vec<String>, RoleSourceError>;

    /// Returns true when `public_key` is an access key of `account_id`.
    ///
    /// This binds the NEP-413 signature to the claimed account. Without this check, anyone could
    /// sign a login message with their own key while claiming a privileged account id.
    async fn has_access_key(
        &self,
        account_id: &str,
        public_key: &str,
    ) -> Result<bool, RoleSourceError>;
}

/// Role source backed by the NEAR JSON-RPC `query` method.
pub struct NearRpcRoleSource {
    client: reqwest::Client,
    rpc_url: String,
    contract_id: String,
}

impl NearRpcRoleSource {
    pub fn new(rpc_url: impl Into<String>, contract_id: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("failed to build the NEAR RPC HTTP client"),
            rpc_url: rpc_url.into(),
            contract_id: contract_id.into(),
        }
    }

    pub fn with_client(
        client: reqwest::Client,
        rpc_url: impl Into<String>,
        contract_id: impl Into<String>,
    ) -> Self {
        Self {
            client,
            rpc_url: rpc_url.into(),
            contract_id: contract_id.into(),
        }
    }

    async fn query(&self, params: Value) -> Result<Value, RoleSourceError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": "rbac-oidc",
            "method": "query",
            "params": params,
        });
        let response = self
            .client
            .post(&self.rpc_url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;
        let value: Value = response.json().await?;
        Ok(value)
    }
}

#[async_trait]
impl RoleSource for NearRpcRoleSource {
    async fn roles_of(&self, account_id: &str) -> Result<Vec<String>, RoleSourceError> {
        let args = STANDARD.encode(
            serde_json::to_vec(&json!({ "account_id": account_id }))
                .map_err(|err| RoleSourceError::Decode(err.to_string()))?,
        );
        let value = self
            .query(json!({
                "request_type": "call",
                "finality": "final",
                "account_id": self.contract_id,
                "method_name": "roles_of",
                "args_base64": args,
            }))
            .await?;

        if let Some(error) = value.get("error") {
            return Err(RoleSourceError::Rpc(error.to_string()));
        }

        let result = value
            .pointer("/result/result")
            .ok_or_else(|| RoleSourceError::Unexpected(value.to_string()))?;
        let bytes = decode_result_bytes(result)?;
        let mut roles: Vec<String> = serde_json::from_slice(&bytes)
            .map_err(|err| RoleSourceError::Decode(err.to_string()))?;
        roles.sort();
        roles.dedup();
        Ok(roles)
    }

    async fn has_access_key(
        &self,
        account_id: &str,
        public_key: &str,
    ) -> Result<bool, RoleSourceError> {
        let value = self
            .query(json!({
                "request_type": "view_access_key",
                "finality": "final",
                "account_id": account_id,
                "public_key": public_key,
            }))
            .await?;

        if value.pointer("/result/nonce").is_some() {
            return Ok(true);
        }

        if let Some(error) = value.get("error") {
            let error = error.to_string();
            // These are expected answers for "no such key" and "no such account".
            if error.contains("UNKNOWN_ACCESS_KEY")
                || error.contains("UNKNOWN_ACCOUNT")
                || error.contains("does not exist")
            {
                return Ok(false);
            }
            return Err(RoleSourceError::Rpc(error));
        }

        Err(RoleSourceError::Unexpected(value.to_string()))
    }
}

fn decode_result_bytes(value: &Value) -> Result<Vec<u8>, RoleSourceError> {
    match value {
        Value::Array(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let byte = item
                    .as_u64()
                    .filter(|byte| *byte <= u8::MAX as u64)
                    .ok_or_else(|| {
                        RoleSourceError::Unexpected(format!("invalid result byte {item}"))
                    })?;
                bytes.push(byte as u8);
            }
            Ok(bytes)
        }
        Value::String(encoded) => STANDARD
            .decode(encoded)
            .map_err(|err| RoleSourceError::Decode(err.to_string())),
        other => Err(RoleSourceError::Unexpected(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_byte_array_results() {
        let value = json!([123, 125]);
        assert_eq!(decode_result_bytes(&value).unwrap(), b"{}");
    }

    #[test]
    fn decodes_base64_results() {
        let value = json!(STANDARD.encode(b"[\"admin\"]"));
        assert_eq!(decode_result_bytes(&value).unwrap(), b"[\"admin\"]");
    }

    #[test]
    fn rejects_invalid_byte_arrays() {
        assert!(decode_result_bytes(&json!([256])).is_err());
        assert!(decode_result_bytes(&json!(["x"])).is_err());
        assert!(decode_result_bytes(&json!({"x": 1})).is_err());
    }
}
