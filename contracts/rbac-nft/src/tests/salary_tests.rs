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
    let _ = contract.burn_role(user(), "employee".to_owned(), Some(false));

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
    let _ = contract.burn_role(user(), "employee".to_owned(), Some(false));

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
fn holders_of_the_same_role_accrue_independently() {
    let mut contract = new_contract();
    set_env(owner(), 0);
    contract.set_role_salary("employee".to_owned(), U128(100));
    contract.mint_role(user(), "employee".to_owned());

    set_env(owner(), 2 * INTERVAL_NS);
    contract.mint_role(account("bob.test.near"), "employee".to_owned());

    set_env(user(), 4 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(400));
    assert_eq!(
        contract.claimable(account("bob.test.near")),
        U128(200),
        "bob's accrual starts at his own mint time"
    );

    expect_promise(contract.claim(user()));
    set_env(user(), 4 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(0));
    assert_eq!(
        contract.claimable(account("bob.test.near")),
        U128(200),
        "alice's claim does not move bob's timestamp"
    );
}

#[test]
fn burning_one_holder_does_not_affect_other_holders() {
    let mut contract = new_contract();
    create_role(&mut contract, "employee", 100);
    set_env(owner(), 0);
    contract.mint_role(user(), "employee".to_owned());
    contract.mint_role(account("bob.test.near"), "employee".to_owned());

    set_env(owner(), 2 * INTERVAL_NS);
    let _ = contract.burn_role(user(), "employee".to_owned(), Some(false));

    set_env(owner(), 2 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(0));
    assert_eq!(
        contract.claimable(account("bob.test.near")),
        U128(200),
        "the remaining holder keeps accruing"
    );
    assert_eq!(
        contract.members_of("employee".to_owned(), None, None),
        vec![account("bob.test.near")]
    );
    assert_eq!(contract.get_role("employee".to_owned()).unwrap().holders, 1);
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

#[test]
fn burn_pays_pending_salary_by_default() {
    let mut contract = setup(100);
    set_env(owner(), 5 * INTERVAL_NS);

    match contract.burn_role(user(), "employee".to_owned(), None) {
        PromiseOrValue::Promise(_) => {}
        PromiseOrValue::Value(_) => panic!("expected a salary transfer"),
    }
    assert!(
        contract.has_role(user(), "employee".to_owned()),
        "membership remains until the payout succeeds"
    );
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(5 * INTERVAL_NS)),
        "the timestamp advances so the member cannot double-claim"
    );

    let receipts = get_receipts();
    let (_, args) = receipt_function_call(&receipts[0]).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["receiver_id"], user().to_string());
    assert_eq!(args["amount"].as_str().unwrap(), "500");
    assert_eq!(
        receipt_methods(&receipts[1]),
        vec!["on_burn_transfer".to_owned()]
    );

    set_env_with_results(
        contract_id(),
        5 * INTERVAL_NS,
        vec![PromiseResult::Successful(Vec::new())],
    );
    assert!(contract.on_burn_transfer(
        user(),
        U64(role_id("employee")),
        U64(0),
        U128(500),
        Some(owner()),
    ));
    assert!(!contract.has_role(user(), "employee".to_owned()));
    assert_eq!(contract.last_claim(user(), "employee".to_owned()), None);
    assert!(contract
        .members_of("employee".to_owned(), None, None)
        .is_empty());
}

#[test]
fn failed_burn_payout_keeps_membership_and_restores_the_timestamp() {
    let mut contract = setup(100);
    set_env(owner(), 5 * INTERVAL_NS);
    match contract.burn_role(user(), "employee".to_owned(), None) {
        PromiseOrValue::Promise(_) => {}
        PromiseOrValue::Value(_) => panic!("expected a salary transfer"),
    }

    set_env_with_results(contract_id(), 5 * INTERVAL_NS, vec![PromiseResult::Failed]);
    assert!(!contract.on_burn_transfer(
        user(),
        U64(role_id("employee")),
        U64(0),
        U128(500),
        Some(owner()),
    ));
    assert!(contract.has_role(user(), "employee".to_owned()));
    assert_eq!(
        contract.last_claim(user(), "employee".to_owned()),
        Some(U64(0))
    );
    set_env(user(), 5 * INTERVAL_NS);
    assert_eq!(contract.claimable(user()), U128(500));
}

#[test]
fn burn_can_explicitly_forfeit_pending_salary() {
    let mut contract = setup(100);
    set_env(owner(), 5 * INTERVAL_NS);
    match contract.burn_role(user(), "employee".to_owned(), Some(false)) {
        PromiseOrValue::Value(ok) => assert!(ok),
        PromiseOrValue::Promise(_) => panic!("forfeiting should not create a transfer"),
    }
    assert!(!contract.has_role(user(), "employee".to_owned()));
    assert!(get_receipts().is_empty());
}

#[test]
fn burning_without_pending_salary_removes_the_role_immediately() {
    let mut contract = setup(100);
    set_env(owner(), INTERVAL_NS - 1);
    match contract.burn_role(user(), "employee".to_owned(), None) {
        PromiseOrValue::Value(ok) => assert!(ok),
        PromiseOrValue::Promise(_) => panic!("nothing is due yet"),
    }
    assert!(!contract.has_role(user(), "employee".to_owned()));
}

#[test]
fn liability_views_sum_accrued_salary_for_all_holders() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 50, 0);
    grant_role(&mut contract, "bob.test.near", "employee", 50, 0);

    set_env(owner(), 3 * INTERVAL_NS);
    assert_eq!(
        contract.accrued_liabilities_for_role("admin".to_owned()),
        U128(300)
    );
    assert_eq!(
        contract.accrued_liabilities_for_role("employee".to_owned()),
        U128(300),
        "both holders accrue independently"
    );
    assert_eq!(contract.accrued_liabilities(), U128(600));
    assert_eq!(
        contract.accrued_liabilities_for_role("unknown".to_owned()),
        U128(0)
    );
}

#[test]
fn claim_reduces_accrued_liabilities() {
    let mut contract = setup(100);
    set_env(user(), 4 * INTERVAL_NS);
    assert_eq!(contract.accrued_liabilities(), U128(400));
    expect_promise(contract.claim(user()));

    set_env(user(), 4 * INTERVAL_NS);
    assert_eq!(contract.accrued_liabilities(), U128(0));
}

#[test]
fn owner_can_withdraw_from_any_token() {
    let mut contract = new_contract();
    let token = account("stranded.test.near");
    let receiver = account("treasury.test.near");

    set_env_with_deposit(OWNER_ID, 0, near_sdk::NearToken::from_yoctonear(1));
    let _ = contract.withdraw_ft(token.clone(), receiver.clone(), U128(123));

    let receipts = get_receipts();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].receiver_id, token);
    assert_eq!(
        receipt_methods(&receipts[0]),
        vec!["ft_transfer".to_owned()]
    );
    let (_, args) = receipt_function_call(&receipts[0]).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["receiver_id"], receiver.to_string());
    assert_eq!(args["amount"].as_str().unwrap(), "123");
}

#[test]
#[should_panic(expected = "only the contract owner")]
fn non_owner_cannot_withdraw_tokens() {
    let mut contract = new_contract();
    set_env_with_deposit(
        "mallory.test.near",
        0,
        near_sdk::NearToken::from_yoctonear(1),
    );
    let _ = contract.withdraw_ft(
        account("token.test.near"),
        account("mallory.test.near"),
        U128(1),
    );
}

#[test]
#[should_panic(expected = "attach at least 1 yoctoNEAR")]
fn withdraw_requires_a_yocto_deposit() {
    let mut contract = new_contract();
    set_env_with_deposit(OWNER_ID, 0, near_sdk::NearToken::from_yoctonear(0));
    let _ = contract.withdraw_ft(
        account("token.test.near"),
        account("treasury.test.near"),
        U128(1),
    );
}

#[test]
#[should_panic(expected = "amount must be positive")]
fn withdraw_rejects_zero() {
    let mut contract = new_contract();
    set_env_with_deposit(OWNER_ID, 0, near_sdk::NearToken::from_yoctonear(1));
    let _ = contract.withdraw_ft(
        account("token.test.near"),
        account("treasury.test.near"),
        U128(0),
    );
}
