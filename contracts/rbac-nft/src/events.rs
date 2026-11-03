//! NEP-297 event helpers for the RBAC role NFTs.
//!
//! The contract emits standard `nep245` `mt_mint` and `mt_burn` events. Role configuration and
//! salary changes are intentionally not NEP-245 token events; indexers can read them from state.

use near_sdk::serde::Serialize;
use near_sdk::{env, AccountId};

const STANDARD: &str = "nep245";
const VERSION: &str = "1.0.0";

#[derive(Serialize)]
struct EventEnvelope<T> {
    standard: &'static str,
    version: &'static str,
    event: &'static str,
    data: Vec<T>,
}

#[derive(Serialize)]
struct MintEvent {
    owner_id: AccountId,
    token_ids: Vec<String>,
    amounts: Vec<String>,
}

#[derive(Serialize)]
struct BurnEvent {
    owner_id: AccountId,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorized_id: Option<AccountId>,
    token_ids: Vec<String>,
    amounts: Vec<String>,
}

fn emit<T: Serialize>(event: &'static str, data: Vec<T>) {
    let envelope = EventEnvelope {
        standard: STANDARD,
        version: VERSION,
        event,
        data,
    };
    // Serialization of these small, statically-shaped structs cannot fail.
    let json = near_sdk::serde_json::to_string(&envelope)
        .unwrap_or_else(|_| env::panic_str("failed to serialize NEP-297 event"));
    env::log_str(&format!("EVENT_JSON:{json}"));
}

pub(crate) fn emit_mint(owner_id: &AccountId, token_id: &str) {
    emit(
        "mt_mint",
        vec![MintEvent {
            owner_id: owner_id.clone(),
            token_ids: vec![token_id.to_owned()],
            amounts: vec!["1".to_owned()],
        }],
    );
}

pub(crate) fn emit_burn(owner_id: &AccountId, token_id: &str, authorized_id: Option<AccountId>) {
    emit(
        "mt_burn",
        vec![BurnEvent {
            owner_id: owner_id.clone(),
            authorized_id,
            token_ids: vec![token_id.to_owned()],
            amounts: vec!["1".to_owned()],
        }],
    );
}
