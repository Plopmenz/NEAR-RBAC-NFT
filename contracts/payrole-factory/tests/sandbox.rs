#![cfg(feature = "sandbox")]

//! End-to-end factory tests against a real NEAR sandbox.
//!
//! These tests need Docker and prebuilt wasm artifacts. Build them first:
//!
//! ```text
//! ./scripts/build-contract.sh
//! cargo test -p payrole-factory --features sandbox --test sandbox
//! ```
//!
//! The unit tests cover the registry bookkeeping with the mocked blockchain; these tests verify
//! that subaccount deployment, child initialization, owner synchronization and reserve refunds
//! work against a real runtime.

use anyhow::{Context, Result};
use near_sdk::json_types::Base64VecU8;
use near_workspaces::network::Sandbox;
use near_workspaces::types::{Gas, NearToken};
use near_workspaces::{AccountId, Contract, Worker};
use serde_json::json;

/// Storage price used by the sandbox (1e19 yoctoNEAR per byte).
const STORAGE_BYTE_COST: u128 = 10_000_000_000_000_000_000;

fn wasm(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../res")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|err| {
        panic!(
            "failed to read {}: {err}; run ./scripts/build-contract.sh first",
            path.display()
        )
    })
}

async fn deploy_factory(worker: &Worker<Sandbox>, child_code: Vec<u8>) -> Result<Contract> {
    let factory = worker.dev_deploy(&wasm("payrole_factory.wasm")).await?;
    let storage = NearToken::from_yoctonear(child_code.len() as u128 * STORAGE_BYTE_COST);
    factory
        .call("new")
        .args_json(json!({ "code": Base64VecU8::from(child_code) }))
        .deposit(storage.saturating_add(NearToken::from_near(1)))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;
    Ok(factory)
}

async fn deploy_mock_ft(worker: &Worker<Sandbox>) -> Result<Contract> {
    let ft = worker.dev_deploy(&wasm("mock_ft.wasm")).await?;
    ft.call("new")
        .args_json(json!({ "seed": 1 }))
        .transact()
        .await?
        .into_result()?;
    Ok(ft)
}

async fn required_deploy_deposit(factory: &Contract) -> Result<NearToken> {
    let required: String = factory.view("required_deploy_deposit").await?.json()?;
    Ok(NearToken::from_yoctonear(required.parse()?))
}

#[tokio::test]
async fn deploy_registers_the_child_and_tracks_owner_changes() -> Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let ft = deploy_mock_ft(&worker).await?;
    let factory = deploy_factory(&worker, wasm("payrole_nft.wasm")).await?;
    let deployer = worker.dev_create_account().await?;
    let owner = worker.dev_create_account().await?;
    let new_owner = worker.dev_create_account().await?;

    let deposit = required_deploy_deposit(&factory).await?;
    deployer
        .call(factory.id(), "deploy")
        .args_json(json!({
            "name": "acme",
            "owner_id": owner.id(),
            "salary_token": ft.id(),
            "payout_interval_ns": "2592000000000000",
        }))
        .deposit(deposit.saturating_add(NearToken::from_yoctonear(50_000_000_000_000_000_000_000)))
        .gas(Gas::from_tgas(250))
        .transact()
        .await?
        .into_result()?;

    let child: AccountId = format!("acme.{}", factory.id()).parse()?;
    let record: serde_json::Value = factory
        .view("get_contract")
        .args_json(json!({ "account_id": child }))
        .await?
        .json()?;
    assert_eq!(record["owner_id"], owner.id().as_str());
    assert_eq!(record["deployer_id"], deployer.id().as_str());

    let count: u64 = factory.view("contract_count").await?.json()?;
    assert_eq!(count, 1);

    let child_owner: AccountId = worker
        .view(&child, "owner_id")
        .await?
        .json()
        .context("initialized child should expose its owner")?;
    assert_eq!(child_owner, *owner.id());

    let contracts: Vec<serde_json::Value> = factory
        .view("contracts_of_owner")
        .args_json(json!({ "owner_id": owner.id() }))
        .await?
        .json()?;
    assert_eq!(contracts.len(), 1);
    assert_eq!(contracts[0]["account_id"], child.as_str());

    // The child notifies the factory when its owner changes. Factory-deployed children have no
    // keys, so the current owner drives the call.
    owner
        .call(&child, "set_owner")
        .args_json(json!({ "new_owner_id": new_owner.id() }))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;

    let record: serde_json::Value = factory
        .view("get_contract")
        .args_json(json!({ "account_id": child }))
        .await?
        .json()?;
    assert_eq!(record["owner_id"], new_owner.id().as_str());

    let old_contracts: Vec<serde_json::Value> = factory
        .view("contracts_of_owner")
        .args_json(json!({ "owner_id": owner.id() }))
        .await?
        .json()?;
    assert!(
        old_contracts.is_empty(),
        "old owner index should be cleared"
    );

    let new_contracts: Vec<serde_json::Value> = factory
        .view("contracts_of_owner")
        .args_json(json!({ "owner_id": new_owner.id() }))
        .await?
        .json()?;
    assert_eq!(new_contracts.len(), 1);
    assert_eq!(new_contracts[0]["account_id"], child.as_str());
    Ok(())
}

#[tokio::test]
async fn failed_child_initialization_is_not_registered_and_refunds_the_reserve() -> Result<()> {
    let worker = near_workspaces::sandbox().await?;
    // The mock FT's `new(seed)` does not accept the PayRole init arguments, so the child init fails.
    let factory = deploy_factory(&worker, wasm("mock_ft.wasm")).await?;
    let deployer = worker.dev_create_account().await?;

    let before = factory.view_account().await?.balance;
    let deposit = required_deploy_deposit(&factory).await?;
    let outcome = deployer
        .call(factory.id(), "deploy")
        .args_json(json!({
            "name": "broken",
            "owner_id": deployer.id(),
            "salary_token": deployer.id(),
            "payout_interval_ns": "2592000000000000",
        }))
        .deposit(deposit)
        .gas(Gas::from_tgas(250))
        .transact()
        .await?
        .into_result()?;

    assert!(
        outcome
            .logs()
            .iter()
            .any(|log| log.contains("deploy_failed")),
        "the factory should emit a failure event: {:?}",
        outcome.logs()
    );
    let count: u64 = factory.view("contract_count").await?.json()?;
    assert_eq!(count, 0, "a failed child must not enter the registry");

    // The factory received the attached deposit but refunded its registry reserve, so its
    // balance should not grow. The forwarded child balance is burned with the account.
    let after = factory.view_account().await?.balance;
    assert!(
        after <= before,
        "factory balance grew after a failed deploy: {before:?} -> {after:?}"
    );
    Ok(())
}
