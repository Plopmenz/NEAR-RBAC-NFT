//! Tests for salary accrual, claims, callbacks and payroll configuration changes.

use near_sdk::json_types::{U128, U64};
use near_sdk::{PromiseOrValue, PromiseResult};
use rbac_common::role_id;

use super::helpers::*;
use crate::Contract;

fn setup(salary: u128) -> Contract {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "employee", salary, 0);
    contract
}

fn claimable(contract: &Contract, timestamp: u64) -> u128 {
    set_env(user(), timestamp);
    contract.claimable(user()).0
}

fn expect_value(value: PromiseOrValue<U128>) -> u128 {
    match value {
        PromiseOrValue::Value(value) => value.0,
        PromiseOrValue::Promise(_) => panic!("expected an immediate value, got a promise"),
    }
}

fn expect_promise(value: PromiseOrValue<U128>) {
    match value {
        PromiseOrValue::Value(value) => panic!("expected a promise, got value {}", value.0),
        PromiseOrValue::Promise(_) => {}
    }
}

#[test]
fn salary_starts_at_mint_time() {
    let mut contract = new_contract();
    set_env(owner(), 0);
    contract.set_role_salary("employee".to_owned(), U128(100));
    set_env(owner(), 10 * INTERVAL_NS);
    contract.mint_role(user(), "employee".to_owned());

    assert_eq!(claimable(&contract, 10 * INTERVAL_NS), 0);
    assert_eq!(claimable(&contract, 11 * INTERVAL_NS - 1), 0);
    assert_eq!(claimable(&contract, 11 * INTERVAL_NS), 100);
    assert_eq!(claimable(&contract, 12 * INTERVAL_NS), 200);
}

#[test]
fn salary_accrues_in_full_intervals_only() {
    let contract = setup(100);
    assert_eq!(claimable(&contract, INTERVAL_NS - 1), 0);
    assert_eq!(claimable(&contract, INTERVAL_NS), 100);
    assert_eq!(claimable(&contract, 2 * INTERVAL_NS - 1), 100);
    assert_eq!(claimable(&contract, 3 * INTERVAL_NS + INTERVAL_NS / 2), 300);
}

#[test]
fn claimable_is_zero_for_unknown_accounts_and_roles() {
    let contract = setup(100);
    set_env(owner(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(account("nobody.test.near")), U128(0));
    assert_eq!(
        contract.claimable_role(user(), "unknown".to_owned()),
        U128(0)
    );
}

#[test]
fn claim_returns_zero_when_nothing_is_due() {
    let mut contract = setup(100);
    set_env(user(), INTERVAL_NS - 1);
    assert_eq!(expect_value(contract.claim(user())), 0);
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(0))
    );
}

#[test]
fn claim_returns_zero_for_an_account_without_roles() {
    let mut contract = new_contract();
    set_env(user(), 10 * INTERVAL_NS);
    assert_eq!(expect_value(contract.claim(user())), 0);
}

#[test]
fn claim_pays_full_intervals_and_advances_the_timestamp() {
    let mut contract = setup(100);
    set_env(user(), 3 * INTERVAL_NS + 700);

    expect_promise(contract.claim(user()));
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(3 * INTERVAL_NS))
    );
    assert_eq!(contract.claimable(user()), U128(0));

    set_env(user(), 4 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(100));
}

#[test]
fn claim_creates_a_transfer_and_a_private_callback() {
    let mut contract = setup(250);
    set_env(account("trigger.test.near"), 2 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    let receipts = get_receipts();
    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].receiver_id, salary_token());
    assert_eq!(
        receipt_methods(&receipts[0]),
        vec!["ft_transfer".to_owned()]
    );
    assert_eq!(receipts[1].receiver_id, contract_id());
    assert_eq!(
        receipt_methods(&receipts[1]),
        vec!["on_claim_transfer".to_owned()]
    );

    let (method, args) = receipt_function_call(&receipts[0]).unwrap();
    assert_eq!(method, "ft_transfer");
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["receiver_id"], user().to_string());
    assert_eq!(args["amount"].as_str().unwrap(), "500");

    let (method, args) = receipt_function_call(&receipts[1]).unwrap();
    assert_eq!(method, "on_claim_transfer");
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["account_id"], user().to_string());
}

#[test]
fn claim_role_claims_only_one_role() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 50, 0);

    set_env(user(), 2 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(300));
    expect_promise(contract.claim_role(user(), "admin".to_owned()));

    assert_eq!(
        contract.last_claim(user(), "admin".to_owned()),
        Some(U64(2 * INTERVAL_NS))
    );
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(0))
    );
    assert_eq!(contract.claimable(user()), U128(100));
}

#[test]
fn claim_combines_multiple_roles() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 50, 0);

    set_env(user(), 3 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    let receipts = get_receipts();
    let (_, args) = receipt_function_call(&receipts[0]).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["amount"].as_str().unwrap(), "450");
    assert_eq!(
        contract.last_claim(user(), "admin".to_owned()),
        Some(U64(3 * INTERVAL_NS))
    );
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(3 * INTERVAL_NS))
    );
}

#[test]
fn zero_salary_roles_advance_their_claim_timestamp_without_a_transfer() {
    let mut contract = setup(0);
    set_env(user(), 2 * INTERVAL_NS);
    assert_eq!(expect_value(contract.claim(user())), 0);
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(2 * INTERVAL_NS))
    );
    assert!(get_receipts().is_empty());
}

#[test]
fn salary_changes_apply_retroactively_to_unclaimed_time() {
    let mut contract = setup(100);
    set_env(user(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(500));

    set_env(owner(), 5 * INTERVAL_NS);
    contract.set_role_salary("employee".to_owned(), U128(200));

    set_env(user(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(1_000));
    expect_promise(contract.claim(user()));
    let receipts = get_receipts();
    let (_, args) = receipt_function_call(&receipts[0]).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["amount"].as_str().unwrap(), "1000");
}

#[test]
fn payout_interval_changes_apply_retroactively_to_unclaimed_time() {
    let mut contract = setup(100);
    set_env(user(), 10 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(1_000));

    set_env(owner(), 10 * INTERVAL_NS);
    contract.set_payout_config(salary_token(), U64(2 * INTERVAL_NS));

    set_env(user(), 10 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(500));
}

#[test]
fn burn_forfeits_unclaimed_salary_and_stops_accrual() {
    let mut contract = setup(100);
    set_env(user(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(500));

    set_env(owner(), 5 * INTERVAL_NS);
    contract.burn_role(user(), "employee".to_owned());

    set_env(user(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(0));
    assert_eq!(contract.last_claim(user(), "employee".to_owned()), None);

    set_env(user(), 20 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(0));
    assert_eq!(expect_value(contract.claim(user())), 0);
}

#[test]
fn claim_callback_keeps_timestamps_on_transfer_success() {
    let mut contract = setup(100);
    set_env(user(), 3 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    set_env_with_results(
        contract_id(),
        3 * INTERVAL_NS,
        vec![PromiseResult::Successful(Vec::new())],
    );
    let ok = contract.on_claim_transfer(user(), vec![(role_id("employee"), 0)], U128(300));
    assert!(ok);
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(3 * INTERVAL_NS))
    );
    assert_eq!(contract.claimable(user()), U128(0));
}

#[test]
fn claim_callback_restores_timestamps_on_transfer_failure() {
    let mut contract = setup(100);
    set_env(user(), 3 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    set_env_with_results(contract_id(), 3 * INTERVAL_NS, vec![PromiseResult::Failed]);
    let ok = contract.on_claim_transfer(user(), vec![(role_id("employee"), 0)], U128(300));
    assert!(!ok);
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(0))
    );
    set_env(user(), 3 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(300));
}

#[test]
fn claim_callback_restores_all_roles_on_failure() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 50, 0);

    set_env(user(), 2 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    set_env_with_results(contract_id(), 2 * INTERVAL_NS, vec![PromiseResult::Failed]);
    let ok = contract.on_claim_transfer(
        user(),
        vec![(role_id("admin"), 0), (role_id("employee"), 0)],
        U128(300),
    );
    assert!(!ok);
    assert_eq!(
        contract.last_claim(user(), "admin".to_owned()),
        Some(U64(0))
    );
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(0))
    );
    assert_eq!(contract.claimable(user()), U128(300));
}

#[test]
fn claim_callback_skips_roles_burned_while_the_transfer_was_in_flight() {
    let mut contract = setup(100);
    set_env(user(), 2 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    set_env(owner(), 2 * INTERVAL_NS);
    contract.burn_role(user(), "employee".to_owned());

    set_env_with_results(contract_id(), 2 * INTERVAL_NS, vec![PromiseResult::Failed]);
    let ok = contract.on_claim_transfer(user(), vec![(role_id("employee"), 0)], U128(200));
    assert!(!ok);
    assert_eq!(contract.last_claim(user(), "employee".to_owned()), None);
}

#[test]
fn claim_callback_supports_non_empty_success_payloads() {
    let mut contract = setup(100);
    set_env(user(), INTERVAL_NS);
    expect_promise(contract.claim(user()));

    set_env_with_results(
        contract_id(),
        INTERVAL_NS,
        vec![PromiseResult::Successful(b"ok".to_vec())],
    );
    assert!(contract.on_claim_transfer(user(), vec![(role_id("employee"), 0)], U128(100)));
}

#[test]
#[should_panic(expected = "expected exactly one promise result")]
fn claim_callback_requires_a_promise_result() {
    let mut contract = setup(100);
    set_env(contract_id(), INTERVAL_NS);
    contract.on_claim_transfer(user(), Vec::new(), U128(0));
}

#[test]
#[should_panic(expected = "expected exactly one promise result")]
fn claim_callback_rejects_multiple_promise_results() {
    let mut contract = setup(100);
    set_env_with_results(
        contract_id(),
        INTERVAL_NS,
        vec![
            PromiseResult::Successful(Vec::new()),
            PromiseResult::Successful(Vec::new()),
        ],
    );
    contract.on_claim_transfer(user(), Vec::new(), U128(0));
}

#[test]
fn last_claim_is_none_for_unknown_tokens() {
    let contract = new_contract();
    assert_eq!(
        contract.last_claim(account("nobody.test.near"), "admin".to_owned()),
        None
    );
}

#[test]
fn anyone_can_trigger_a_claim_but_payment_goes_to_the_member() {
    let mut contract = setup(100);
    set_env(account("keeper.test.near"), 2 * INTERVAL_NS);
    expect_promise(contract.claim(user()));

    let receipts = get_receipts();
    let (_, args) = receipt_function_call(&receipts[0]).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["receiver_id"], user().to_string());
}
