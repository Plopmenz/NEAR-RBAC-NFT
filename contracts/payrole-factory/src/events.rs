//! NEP-297 events emitted by the factory.
//!
//! Events use the custom `payrole-factory` standard. The payload is a full
//! [`ContractRecord`](crate::ContractRecord), so indexers can follow deployments without reading
//! contract state.

use near_sdk::serde::Serialize;
use near_sdk::{env, serde_json};

use crate::ContractRecord;

const STANDARD: &str = "payrole-factory";
const VERSION: &str = "1.0.0";

#[derive(Serialize)]
struct EventEnvelope<T> {
    standard: &'static str,
    version: &'static str,
    event: &'static str,
    data: Vec<T>,
}

#[derive(Serialize)]
struct FailureEvent<'a> {
    account_id: &'a str,
    reason: &'a str,
}

fn emit<T: Serialize>(event: &'static str, data: Vec<T>) {
    let envelope = EventEnvelope {
        standard: STANDARD,
        version: VERSION,
        event,
        data,
    };
    // Serialization of these small, statically-shaped structs cannot fail.
    let json = serde_json::to_string(&envelope)
        .unwrap_or_else(|_| env::panic_str("failed to serialize NEP-297 event"));
    env::log_str(&format!("EVENT_JSON:{json}"));
}

pub(crate) fn emit_deploy(record: &ContractRecord) {
    emit("deploy", vec![record.clone()]);
}

pub(crate) fn emit_deploy_failed(account_id: &str, reason: &str) {
    emit("deploy_failed", vec![FailureEvent { account_id, reason }]);
}
