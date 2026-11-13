//! Unit tests for the factory contract.

use near_parameters::RuntimeFeesConfig;
use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{get_created_receipts, get_logs, VMContextBuilder};
use near_sdk::{test_vm_config, testing_env, AccountId, Gas, NearToken, PromiseResult, VMContext};

use crate::Contract;

const FACTORY_ID: &str = "factory.test.near";
const OWNER_ID: &str = "dao.test.near";
const DEPLOYER_ID: &str = "alice.test.near";
const SALARY_TOKEN_ID: &str = "salary.test.near";
const INTERVAL_NS: u64 = 604_800_000_000_000;
const TEST_CODE: [u8; 5] = [0x00, 0x61, 0x73, 0x6d, 0x01];

fn account(id: &str) -> AccountId {
    id.parse().expect("valid test account id")
}

fn owner() -> AccountId {
    account(OWNER_ID)
}

fn deployer() -> AccountId {
    account(DEPLOYER_ID)
}

fn salary_token() -> AccountId {
    account(SALARY_TOKEN_ID)
}

fn factory_id() -> AccountId {
    account(FACTORY_ID)
}

fn context(predecessor: &str, deposit: NearToken, gas: Gas) -> VMContext {
    VMContextBuilder::new()
        .current_account_id(factory_id())
        .signer_account_id(account(predecessor))
        .predecessor_account_id(account(predecessor))
        .attached_deposit(deposit)
        .prepaid_gas(gas)
        .account_balance(NearToken::from_near(100))
        .storage_usage(10_000)
        .block_timestamp(1_000_000)
        .build()
}

fn set_env(predecessor: &str, deposit: NearToken) {
    testing_env!(context(predecessor, deposit, Gas::from_tgas(300)));
}

fn set_env_with_results(predecessor: &str, results: Vec<PromiseResult>) {
    testing_env!(
        context(
            predecessor,
            NearToken::from_yoctonear(0),
            Gas::from_tgas(300)
        ),
        test_vm_config(),
        RuntimeFeesConfig::test(),
        Default::default(),
        results
    );
}

fn factory_with_code(code: Vec<u8>) -> Contract {
    set_env(DEPLOYER_ID, NearToken::from_near(1));
    Contract::new(Base64VecU8::from(code))
}

fn new_factory() -> Contract {
    factory_with_code(TEST_CODE.to_vec())
}

fn with_code() -> Contract {
    new_factory()
}

fn deploy(factory: &mut Contract, name: &str) -> near_sdk::Promise {
    set_env(DEPLOYER_ID, NearToken::from_near(10));
    factory.deploy(name.to_owned(), owner(), salary_token(), U64(INTERVAL_NS))
}

fn on_deploy(factory: &mut Contract, results: Vec<PromiseResult>, name: &str) -> bool {
    set_env_with_results(FACTORY_ID, results);
    factory.on_deploy(make_record(name, owner(), deployer(), U64(1_000_000)))
}

fn make_record(
    name: &str,
    owner_id: AccountId,
    deployer_id: AccountId,
    deployed_at_ns: U64,
) -> crate::ContractRecord {
    let account_id: AccountId = format!("{name}.{FACTORY_ID}").parse().unwrap();
    crate::ContractRecord {
        account_id,
        name: name.to_owned(),
        owner_id,
        salary_token: salary_token(),
        payout_interval_ns: U64(INTERVAL_NS),
        deployer_id,
        deployed_at_ns,
    }
}

fn register(factory: &mut Contract, name: &str) {
    assert!(on_deploy(
        factory,
        vec![PromiseResult::Successful(vec![])],
        name
    ));
}

fn record(factory: &Contract, name: &str) -> crate::ContractRecord {
    let id: AccountId = format!("{name}.{FACTORY_ID}").parse().unwrap();
    factory.get_contract(id).expect("record should exist")
}

#[test]
fn initializes_factory_with_immutable_code() {
    let factory = new_factory();
    assert_eq!(factory.code_len(), U64(TEST_CODE.len() as u64));
    assert_eq!(factory.contract_count(), 0);
    assert_eq!(
        factory.code_hash(),
        "42fd6e7502a18143f0c206c99f1e0787fbcac6b1c5f9969a4cb0d2f7035d07e3"
    );
    assert_eq!(factory.code_hash(), new_factory().code_hash());
}

#[test]
fn new_requires_storage_deposit() {
    set_env(OWNER_ID, NearToken::from_yoctonear(0));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Contract::new(Base64VecU8::from(TEST_CODE.to_vec()));
    }));
    assert!(result.is_err());
}

#[test]
fn new_rejects_empty_code() {
    set_env(OWNER_ID, NearToken::from_near(1));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Contract::new(Base64VecU8::from(Vec::new()));
    }));
    assert!(result.is_err());
}

#[test]
fn deploy_rejects_invalid_names() {
    for name in ["", "Acme", "-acme", "_acme", "acme-", "acme_", "ac.me"] {
        let mut factory = with_code();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = deploy(&mut factory, name);
        }));
        assert!(result.is_err(), "name {name:?} should be rejected");
    }
}

#[test]
fn deploy_rejects_name_that_is_too_long() {
    let mut factory = with_code();
    let name = "a".repeat(60);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = deploy(&mut factory, &name);
    }));
    assert!(result.is_err());
}

#[test]
fn deploy_rejects_zero_interval() {
    let mut factory = with_code();
    set_env(DEPLOYER_ID, NearToken::from_near(10));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = factory.deploy("acme".to_owned(), owner(), salary_token(), U64(0));
    }));
    assert!(result.is_err());
}

#[test]
fn deploy_rejects_insufficient_gas() {
    let mut factory = with_code();
    testing_env!(context(
        DEPLOYER_ID,
        NearToken::from_near(10),
        Gas::from_tgas(100)
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = factory.deploy("acme".to_owned(), owner(), salary_token(), U64(INTERVAL_NS));
    }));
    assert!(result.is_err());
}

#[test]
fn deploy_rejects_insufficient_deposit() {
    let mut factory = with_code();
    let required = factory.required_deploy_deposit().0;
    testing_env!(context(
        DEPLOYER_ID,
        NearToken::from_yoctonear(required - 1),
        Gas::from_tgas(300)
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = factory.deploy("acme".to_owned(), owner(), salary_token(), U64(INTERVAL_NS));
    }));
    assert!(result.is_err());
}

#[test]
fn deploy_creates_and_initializes_child() {
    let mut factory = with_code();
    let _ = deploy(&mut factory, "acme");

    let receipts = get_created_receipts();
    assert!(!receipts.is_empty());

    let child = receipts
        .iter()
        .find(|receipt| receipt.receiver_id == account("acme.factory.test.near"))
        .expect("child receipt");
    let mut saw_create = false;
    let mut saw_deploy = false;
    let mut saw_transfer = false;
    let mut saw_init = false;

    for action in &child.actions {
        match action {
            MockAction::CreateAccount { .. } => saw_create = true,
            MockAction::DeployContract { code, .. } => {
                saw_deploy = true;
                assert_eq!(code, &vec![0x00, 0x61, 0x73, 0x6d, 0x01]);
            }
            MockAction::Transfer { deposit, .. } => {
                saw_transfer = true;
                assert!(deposit.as_yoctonear() > 0);
            }
            MockAction::FunctionCallWeight {
                method_name, args, ..
            } => {
                assert_eq!(method_name, b"new");
                let args: serde_json::Value = serde_json::from_slice(args).unwrap();
                assert_eq!(args["owner_id"], OWNER_ID);
                assert_eq!(args["salary_token"], SALARY_TOKEN_ID);
                assert_eq!(args["payout_interval_ns"], INTERVAL_NS.to_string());
                assert_eq!(args["factory_id"], FACTORY_ID);
                saw_init = true;
            }
            _ => {}
        }
    }

    assert!(saw_create && saw_deploy && saw_transfer && saw_init);

    let callback = receipts
        .iter()
        .find(|receipt| receipt.receiver_id == factory_id())
        .expect("callback receipt");
    assert!(callback.actions.iter().any(|action| matches!(
        action,
        MockAction::FunctionCallWeight { method_name, .. } if method_name == b"on_deploy"
    )));
}

#[test]
fn on_deploy_registers_successful_child() {
    let mut factory = with_code();
    assert!(on_deploy(
        &mut factory,
        vec![PromiseResult::Successful(vec![])],
        "acme"
    ));

    assert_eq!(factory.contract_count(), 1);
    let record = record(&factory, "acme");
    assert_eq!(record.account_id, account("acme.factory.test.near"));
    assert_eq!(record.name, "acme");
    assert_eq!(record.owner_id, owner());
    assert_eq!(record.salary_token, salary_token());
    assert_eq!(record.payout_interval_ns, U64(INTERVAL_NS));
    assert_eq!(record.deployer_id, deployer());
    assert_eq!(record.deployed_at_ns, U64(1_000_000));
    assert!(get_logs()
        .iter()
        .any(|log| log.contains("EVENT_JSON") && log.contains("\"event\":\"deploy\"")));
}

#[test]
fn on_deploy_failure_does_not_register() {
    let mut factory = with_code();
    assert!(!on_deploy(
        &mut factory,
        vec![PromiseResult::Failed],
        "acme"
    ));

    assert_eq!(factory.contract_count(), 0);
    assert!(get_logs()
        .iter()
        .any(|log| log.contains("\"event\":\"deploy_failed\"")));
}

#[test]
fn on_deploy_is_idempotent_for_duplicate_records() {
    let mut factory = with_code();
    register(&mut factory, "acme");
    assert!(!on_deploy(
        &mut factory,
        vec![PromiseResult::Successful(vec![])],
        "acme"
    ));
    assert_eq!(factory.contract_count(), 1);
}

#[test]
fn deploy_rejects_registered_name() {
    let mut factory = with_code();
    register(&mut factory, "acme");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = deploy(&mut factory, "acme");
    }));
    assert!(result.is_err());
}

#[test]
fn registry_paginates_in_insertion_order() {
    let mut factory = with_code();
    for name in ["one", "two", "three"] {
        register(&mut factory, name);
    }

    let all = factory.get_contracts(None, None);
    assert_eq!(
        all.iter()
            .map(|record| record.name.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two", "three"]
    );

    let page = factory.get_contracts(Some(U128(1)), Some(1));
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].name, "two");

    let page = factory.get_contracts(Some(U128(2)), Some(100));
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].name, "three");

    assert!(factory.get_contracts(Some(U128(99)), None).is_empty());
}

#[test]
fn registry_indexes_current_owner() {
    let mut factory = with_code();
    register(&mut factory, "one");

    // Deploy a second child for a different owner.
    let other_owner = account("other-dao.test.near");
    set_env_with_results(FACTORY_ID, vec![PromiseResult::Successful(vec![])]);
    assert!(factory.on_deploy(make_record(
        "two",
        other_owner.clone(),
        deployer(),
        U64(2_000_000),
    )));

    let by_owner = factory.contracts_of_owner(owner(), None, None);
    assert_eq!(by_owner.len(), 1);
    assert_eq!(by_owner[0].name, "one");

    let by_owner = factory.contracts_of_owner(other_owner, None, None);
    assert_eq!(by_owner.len(), 1);
    assert_eq!(by_owner[0].name, "two");

    assert!(factory
        .contracts_of_owner(account("nobody.test.near"), None, None)
        .is_empty());
}

#[test]
fn on_owner_changed_moves_the_child_between_indexes() {
    let mut factory = with_code();
    register(&mut factory, "acme");

    let new_owner = account("new-dao.test.near");
    let child = account("acme.factory.test.near");
    set_env_with_results(child.as_str(), vec![]);
    assert!(factory.on_owner_changed(new_owner.clone()));

    assert_eq!(record(&factory, "acme").owner_id, new_owner);
    assert!(factory.contracts_of_owner(owner(), None, None).is_empty());
    let moved = factory.contracts_of_owner(new_owner.clone(), None, None);
    assert_eq!(moved.len(), 1);
    assert_eq!(moved[0].account_id, child);

    // A repeated notification is idempotent.
    set_env_with_results(child.as_str(), vec![]);
    assert!(factory.on_owner_changed(new_owner.clone()));
    assert_eq!(factory.contracts_of_owner(new_owner, None, None).len(), 1);
}

#[test]
#[should_panic(expected = "caller is not a registered child")]
fn on_owner_changed_rejects_unknown_callers() {
    let mut factory = with_code();
    set_env_with_results("mallory.test.near", vec![]);
    factory.on_owner_changed(owner());
}

#[test]
fn on_deploy_failure_refunds_the_registry_reserve() {
    let mut factory = with_code();
    set_env_with_results(FACTORY_ID, vec![PromiseResult::Failed]);
    assert!(!factory.on_deploy(make_record("acme", owner(), deployer(), U64(1_000_000))));

    let receipts = get_created_receipts();
    let refund = receipts
        .iter()
        .find(|receipt| receipt.receiver_id == deployer())
        .expect("refund receipt to the deployer");
    let expected =
        near_sdk::env::storage_byte_cost().as_yoctonear() * crate::REGISTRY_RESERVE_BYTES;
    assert_eq!(
        refund
            .actions
            .iter()
            .map(|action| match action {
                MockAction::Transfer { deposit, .. } => deposit.as_yoctonear(),
                _ => 0,
            })
            .sum::<u128>(),
        expected
    );
}

#[test]
fn required_deposit_accounts_for_code_and_reserves() {
    let small = factory_with_code(vec![0u8; 5]).required_deploy_deposit().0;
    let large = factory_with_code(vec![0u8; 1_005])
        .required_deploy_deposit()
        .0;

    assert!(large > small);
    assert_eq!(
        large - small,
        near_sdk::env::storage_byte_cost().as_yoctonear() * 1_000
    );
}

#[test]
fn get_contract_returns_none_for_unknown_account() {
    let factory = new_factory();
    assert_eq!(factory.get_contract(account("unknown.test.near")), None);
}
