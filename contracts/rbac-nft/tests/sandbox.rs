#![cfg(feature = "sandbox")]

//! End-to-end salary tests against a real NEAR sandbox.
//!
//! These tests need Docker and prebuilt wasm artifacts. Build them first:
//!
//! ```text
//! ./scripts/build-contract.sh
//! cargo test -p rbac-nft --features sandbox --test sandbox
//! ```
//!
//! The unit tests cover the same branches with the mocked blockchain; these tests verify that the
//! promise chains, callbacks and NEP-141 calls work against a real runtime.

use std::time::Duration;

use anyhow::{Context, Result};
use near_workspaces::network::Sandbox;
use near_workspaces::types::{Gas, NearToken};
use near_workspaces::{AccountId, Contract, Worker};
use serde_json::json;

/// One second, so a short real-time sleep accrues exactly one or more periods.
const INTERVAL_NS: &str = "1000000000";
const SALARY: &str = "100";

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

async fn deploy_mock_ft(worker: &Worker<Sandbox>) -> Result<Contract> {
    let ft = worker.dev_deploy(&wasm("mock_ft.wasm")).await?;
    ft.call("new")
        .args_json(json!({ "seed": 1 }))
        .transact()
        .await?
        .into_result()?;
    Ok(ft)
}

async fn ft_balance(ft: &Contract, account: &AccountId) -> Result<u128> {
    let value: String = ft
        .view("ft_balance_of")
        .args_json(json!({ "account_id": account }))
        .await?
        .json()?;
    value.parse().context("invalid FT balance")
}

async fn value<T: serde::de::DeserializeOwned>(
    contract: &Contract,
    method: &str,
    args: serde_json::Value,
) -> Result<T> {
    Ok(contract.view(method).args_json(args).await?.json()?)
}

async fn wait_period() {
    // The sandbox produces block timestamps from wall-clock time, so sleeping is enough.
    tokio::time::sleep(Duration::from_millis(1_200)).await;
}

#[tokio::test]
async fn claim_and_burn_pay_salary_through_the_mock_token() -> Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let ft = deploy_mock_ft(&worker).await?;
    let owner = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;

    let contract = worker.dev_deploy(&wasm("rbac_nft.wasm")).await?;
    contract
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "salary_token": ft.id(),
            "payout_interval_ns": INTERVAL_NS,
            "factory_id": null,
        }))
        .transact()
        .await?
        .into_result()?;

    ft.call("mint")
        .args_json(json!({ "account_id": contract.id(), "amount": "1000000" }))
        .transact()
        .await?
        .into_result()?;

    owner
        .call(contract.id(), "set_role_salary")
        .args_json(json!({ "role": "employee", "salary": SALARY }))
        .transact()
        .await?
        .into_result()?;

    let required: String = value(&contract, "required_mint_deposit", json!({})).await?;
    owner
        .call(contract.id(), "mint_role")
        .args_json(json!({ "account_id": alice.id(), "role": "employee" }))
        .deposit(NearToken::from_yoctonear(required.parse()?))
        .transact()
        .await?
        .into_result()?;

    wait_period().await;
    let claimable: String =
        value(&contract, "claimable", json!({ "account_id": alice.id() })).await?;
    assert!(claimable.parse::<u128>()? > 0, "salary should have accrued");

    alice
        .call(contract.id(), "claim")
        .args_json(json!({ "account_id": alice.id() }))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;
    let after_claim = ft_balance(&ft, alice.id()).await?;
    assert!(after_claim > 0, "claim must transfer salary");

    wait_period().await;
    owner
        .call(contract.id(), "burn_role")
        .args_json(json!({
            "account_id": alice.id(),
            "role": "employee",
            "pay_pending": null,
        }))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    let has_role: bool = value(
        &contract,
        "has_role",
        json!({ "account_id": alice.id(), "role": "employee" }),
    )
    .await?;
    assert!(!has_role, "burn should remove the membership");
    let after_burn = ft_balance(&ft, alice.id()).await?;
    assert!(
        after_burn > after_claim,
        "burn should pay the pending salary by default"
    );
    Ok(())
}

#[tokio::test]
async fn failed_burn_payout_keeps_membership_and_restores_claim_time() -> Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let ft = deploy_mock_ft(&worker).await?;
    let owner = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;

    let contract = worker.dev_deploy(&wasm("rbac_nft.wasm")).await?;
    contract
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "salary_token": ft.id(),
            "payout_interval_ns": INTERVAL_NS,
            "factory_id": null,
        }))
        .transact()
        .await?
        .into_result()?;
    ft.call("mint")
        .args_json(json!({ "account_id": contract.id(), "amount": "1000000" }))
        .transact()
        .await?
        .into_result()?;
    owner
        .call(contract.id(), "set_role_salary")
        .args_json(json!({ "role": "employee", "salary": SALARY }))
        .transact()
        .await?
        .into_result()?;
    let required: String = value(&contract, "required_mint_deposit", json!({})).await?;
    owner
        .call(contract.id(), "mint_role")
        .args_json(json!({ "account_id": alice.id(), "role": "employee" }))
        .deposit(NearToken::from_yoctonear(required.parse()?))
        .transact()
        .await?
        .into_result()?;

    wait_period().await;
    ft.call("set_fail_transfers")
        .args_json(json!({ "fail": true }))
        .transact()
        .await?
        .into_result()?;

    owner
        .call(contract.id(), "burn_role")
        .args_json(json!({
            "account_id": alice.id(),
            "role": "employee",
            "pay_pending": null,
        }))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    let has_role: bool = value(
        &contract,
        "has_role",
        json!({ "account_id": alice.id(), "role": "employee" }),
    )
    .await?;
    assert!(has_role, "a failed payout must keep the membership");
    let claimable: String =
        value(&contract, "claimable", json!({ "account_id": alice.id() })).await?;
    assert!(
        claimable.parse::<u128>()? > 0,
        "accrued salary must not be discarded"
    );
    assert_eq!(ft_balance(&ft, alice.id()).await?, 0);

    // The rollback restored the previous claim timestamp, so a retry can still pay out.
    ft.call("set_fail_transfers")
        .args_json(json!({ "fail": false }))
        .transact()
        .await?
        .into_result()?;
    wait_period().await;
    owner
        .call(contract.id(), "burn_role")
        .args_json(json!({
            "account_id": alice.id(),
            "role": "employee",
            "pay_pending": null,
        }))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?
        .into_result()?;

    let has_role: bool = value(
        &contract,
        "has_role",
        json!({ "account_id": alice.id(), "role": "employee" }),
    )
    .await?;
    assert!(!has_role, "the retry should complete the burn");
    assert!(
        ft_balance(&ft, alice.id()).await? > 0,
        "the retry should pay the restored salary"
    );
    Ok(())
}

#[tokio::test]
async fn owner_can_withdraw_an_arbitrary_token_and_read_liabilities() -> Result<()> {
    let worker = near_workspaces::sandbox().await?;
    let ft = deploy_mock_ft(&worker).await?;
    let bonus = deploy_mock_ft(&worker).await?;
    let owner = worker.dev_create_account().await?;
    let alice = worker.dev_create_account().await?;

    let contract = worker.dev_deploy(&wasm("rbac_nft.wasm")).await?;
    contract
        .call("new")
        .args_json(json!({
            "owner_id": owner.id(),
            "salary_token": ft.id(),
            "payout_interval_ns": INTERVAL_NS,
            "factory_id": null,
        }))
        .transact()
        .await?
        .into_result()?;
    owner
        .call(contract.id(), "set_role_salary")
        .args_json(json!({ "role": "employee", "salary": SALARY }))
        .transact()
        .await?
        .into_result()?;
    let required: String = value(&contract, "required_mint_deposit", json!({})).await?;
    owner
        .call(contract.id(), "mint_role")
        .args_json(json!({ "account_id": alice.id(), "role": "employee" }))
        .deposit(NearToken::from_yoctonear(required.parse()?))
        .transact()
        .await?
        .into_result()?;

    wait_period().await;
    let liabilities: String = value(&contract, "accrued_liabilities", json!({})).await?;
    assert!(
        liabilities.parse::<u128>()? > 0,
        "liability view should include accrued salary"
    );

    // The contract has no salary-token balance, but the owner can still recover a different token.
    bonus
        .call("mint")
        .args_json(json!({ "account_id": contract.id(), "amount": "500" }))
        .transact()
        .await?
        .into_result()?;
    let owner_before = ft_balance(&bonus, owner.id()).await?;
    owner
        .call(contract.id(), "withdraw_ft")
        .args_json(json!({
            "token": bonus.id(),
            "receiver_id": owner.id(),
            "amount": "200",
        }))
        .deposit(NearToken::from_yoctonear(1))
        .gas(Gas::from_tgas(100))
        .transact()
        .await?
        .into_result()?;

    assert_eq!(ft_balance(&bonus, contract.id()).await?, 300);
    assert_eq!(ft_balance(&bonus, owner.id()).await?, owner_before + 200);
    Ok(())
}
