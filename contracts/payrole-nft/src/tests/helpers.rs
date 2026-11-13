//! Shared helpers for contract unit tests.
//!
//! Tests use the near-sdk mocked blockchain. Every helper that changes the context resets the
//! mocked blockchain while preserving its storage, then the test calls the contract directly.

use near_parameters::RuntimeFeesConfig;
use near_sdk::json_types::{U128, U64};
use near_sdk::mock::{MockAction, Receipt};
use near_sdk::test_utils::{get_created_receipts, VMContextBuilder};
use near_sdk::{test_vm_config, testing_env, AccountId, NearToken, PromiseResult, VMContext};

use crate::Contract;

pub const CONTRACT_ID: &str = "payrole.test.near";
pub const OWNER_ID: &str = "dao.test.near";
pub const SALARY_TOKEN_ID: &str = "salary.test.near";
pub const INTERVAL_NS: u64 = 1_000_000_000;

pub fn account(id: &str) -> AccountId {
    id.parse().expect("test account id should be valid")
}

pub fn contract_id() -> AccountId {
    account(CONTRACT_ID)
}

pub fn owner() -> AccountId {
    account(OWNER_ID)
}

pub fn salary_token() -> AccountId {
    account(SALARY_TOKEN_ID)
}

pub fn context(predecessor: AccountId, block_timestamp: u64) -> VMContext {
    VMContextBuilder::new()
        .current_account_id(contract_id())
        .signer_account_id(predecessor.clone())
        .predecessor_account_id(predecessor)
        .block_timestamp(block_timestamp)
        // Covers the mint storage deposit for tests that call `mint_role`.
        .attached_deposit(NearToken::from_near(1))
        .build()
}

pub fn set_env(predecessor: AccountId, block_timestamp: u64) {
    testing_env!(context(predecessor, block_timestamp));
}

pub fn set_env_with_deposit(predecessor: &str, block_timestamp: u64, deposit: NearToken) {
    testing_env!(VMContextBuilder::new()
        .current_account_id(contract_id())
        .signer_account_id(account(predecessor))
        .predecessor_account_id(account(predecessor))
        .block_timestamp(block_timestamp)
        .attached_deposit(deposit)
        .build());
}

pub fn set_env_with_results(
    predecessor: AccountId,
    block_timestamp: u64,
    results: Vec<PromiseResult>,
) {
    testing_env!(
        context(predecessor, block_timestamp),
        test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}

/// Creates a contract with the standard test owner, salary token and one-second payout interval.
pub fn new_contract() -> Contract {
    new_contract_with_factory(None)
}

/// Creates a contract that knows the factory that deployed it.
pub fn new_contract_with_factory(factory_id: Option<AccountId>) -> Contract {
    set_env(owner(), 0);
    Contract::new(owner(), salary_token(), U64(INTERVAL_NS), factory_id)
}

/// Creates a role at timestamp zero as the contract owner.
pub fn create_role(contract: &mut Contract, role: &str, salary: u128) {
    set_env(owner(), 0);
    contract.set_role_salary(role.to_owned(), U128(salary));
}

/// Creates a role at timestamp zero and mints it to `member` as the contract owner.
pub fn grant_role(contract: &mut Contract, member: &str, role: &str, salary: u128, timestamp: u64) {
    set_env(owner(), 0);
    contract.set_role_salary(role.to_owned(), U128(salary));
    set_env(owner(), timestamp);
    contract.mint_role(account(member), role.to_owned());
}

pub fn call_as<T>(caller: &str, timestamp: u64, f: impl FnOnce() -> T) -> T {
    set_env(account(caller), timestamp);
    f()
}

pub fn user() -> AccountId {
    account("alice.test.near")
}

pub fn get_receipts() -> Vec<Receipt> {
    get_created_receipts()
}

pub fn receipt_methods(receipt: &Receipt) -> Vec<String> {
    receipt
        .actions
        .iter()
        .filter_map(|action| match action {
            MockAction::FunctionCallWeight { method_name, .. } => {
                Some(String::from_utf8(method_name.clone()).expect("method name is UTF-8"))
            }
            _ => None,
        })
        .collect()
}

#[allow(dead_code)]
pub fn receipt_function_call(receipt: &Receipt) -> Option<(String, Vec<u8>)> {
    receipt.actions.iter().find_map(|action| match action {
        MockAction::FunctionCallWeight {
            method_name, args, ..
        } => Some((
            String::from_utf8(method_name.clone()).expect("method name is UTF-8"),
            args.clone(),
        )),
        _ => None,
    })
}
