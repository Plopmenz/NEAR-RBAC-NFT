//! Tests for configuration, role administration, membership and events.

use near_sdk::json_types::{U128, U64};
use near_sdk::test_utils::get_logs;
use near_sdk::{NearToken, PromiseOrValue};
use payrole_common::{role_id, token_id};

use super::helpers::*;
use crate::{Contract, MAX_ROLES_PER_ACCOUNT};

#[test]
fn initializes_configuration() {
    let contract = new_contract();
    assert_eq!(contract.owner_id(), owner());
    assert_eq!(contract.salary_token(), salary_token());
    assert_eq!(contract.payout_interval_ns(), U64(INTERVAL_NS));
    assert_eq!(contract.role_count(), 0);
}

#[test]
#[should_panic(expected = "payout interval must be positive")]
fn rejects_zero_payout_interval() {
    set_env(owner(), 0);
    Contract::new(owner(), salary_token(), U64(0), None);
}

#[test]
fn owner_can_transfer_ownership() {
    let mut contract = new_contract();
    let new_owner = account("new-dao.test.near");

    let _ = call_as(OWNER_ID, 0, || contract.set_owner(new_owner.clone()));
    assert_eq!(contract.owner_id(), new_owner);

    let _ = call_as("new-dao.test.near", 0, || {
        contract.set_owner(account("another-dao.test.near"))
    });
    assert_eq!(contract.owner_id(), account("another-dao.test.near"));
}

#[test]
#[should_panic(expected = "only the contract owner")]
fn non_owner_cannot_transfer_ownership() {
    let mut contract = new_contract();
    let _ = call_as("mallory.test.near", 0, || {
        contract.set_owner(account("mallory.test.near"))
    });
}

#[test]
fn set_role_salary_creates_a_role() {
    let mut contract = new_contract();
    let view = call_as(OWNER_ID, 0, || {
        contract.set_role_salary("admin".to_owned(), U128(1_000))
    });

    assert_eq!(view.role_id, U64(role_id("admin")));
    assert_eq!(view.name, "admin");
    assert_eq!(view.owner, owner());
    assert_eq!(view.configured_owner, None);
    assert_eq!(view.salary, U128(1_000));
    assert_eq!(view.holders, 0);
    assert_eq!(contract.role_count(), 1);

    assert_eq!(contract.get_role("admin".to_owned()), Some(view.clone()));
    assert_eq!(contract.get_role_by_id(U64(role_id("admin"))), Some(view));
    assert_eq!(contract.get_role("unknown".to_owned()), None);
    assert_eq!(contract.get_role_by_id(U64(0)), None);
}

#[test]
fn set_role_salary_updates_existing_role() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 1_000);

    let updated = call_as(OWNER_ID, 5, || {
        contract.set_role_salary("admin".to_owned(), U128(2_500))
    });

    assert_eq!(updated.salary, U128(2_500));
    assert_eq!(contract.role_count(), 1);
    assert_eq!(
        contract.get_role("admin".to_owned()).unwrap().salary,
        U128(2_500)
    );
}

#[test]
fn role_names_are_case_sensitive() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.set_role_salary("Admin".to_owned(), U128(1));
        contract.set_role_salary("admin".to_owned(), U128(2));
    });

    assert_eq!(contract.role_count(), 2);
    assert_ne!(role_id("Admin"), role_id("admin"));
    assert_eq!(
        contract.get_role("Admin".to_owned()).unwrap().salary,
        U128(1)
    );
    assert_eq!(
        contract.get_role("admin".to_owned()).unwrap().salary,
        U128(2)
    );
}

#[test]
#[should_panic(expected = "only the contract owner")]
fn non_owner_cannot_create_or_price_roles() {
    let mut contract = new_contract();
    call_as("mallory.test.near", 0, || {
        contract.set_role_salary("admin".to_owned(), U128(1))
    });
}

#[test]
#[should_panic(expected = "invalid role name")]
fn rejects_empty_role_name() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.set_role_salary(String::new(), U128(1))
    });
}

#[test]
#[should_panic(expected = "invalid role name")]
fn rejects_role_names_with_surrounding_whitespace() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.set_role_salary(" admin".to_owned(), U128(1))
    });
}

#[test]
#[should_panic(expected = "invalid role name")]
fn rejects_too_long_role_names() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.set_role_salary("a".repeat(65), U128(1))
    });
}

#[test]
fn role_owner_can_be_configured_and_cleared() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    let role_owner = account("hr.test.near");

    let view = call_as(OWNER_ID, 0, || {
        contract.set_role_owner("admin".to_owned(), Some(role_owner.clone()))
    });
    assert_eq!(view.owner, role_owner);
    assert_eq!(view.configured_owner, Some(role_owner.clone()));
    assert_eq!(
        contract.get_role("admin".to_owned()).unwrap().owner,
        role_owner
    );

    let cleared = call_as("hr.test.near", 0, || {
        contract.set_role_owner("admin".to_owned(), None)
    });
    assert_eq!(cleared.owner, owner());
    assert_eq!(cleared.configured_owner, None);
}

#[test]
fn role_owner_can_delegate_role_ownership() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    call_as(OWNER_ID, 0, || {
        contract.set_role_owner("admin".to_owned(), Some(account("first.test.near")))
    });

    call_as("first.test.near", 0, || {
        contract.set_role_owner("admin".to_owned(), Some(account("second.test.near")))
    });

    assert_eq!(
        contract.get_role("admin".to_owned()).unwrap().owner,
        account("second.test.near")
    );
}

#[test]
#[should_panic(expected = "only the contract owner or role owner")]
fn unrelated_account_cannot_change_role_owner() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    call_as(OWNER_ID, 0, || {
        contract.set_role_owner("admin".to_owned(), Some(account("hr.test.near")))
    });

    call_as("mallory.test.near", 0, || {
        contract.set_role_owner("admin".to_owned(), Some(account("mallory.test.near")))
    });
}

#[test]
fn owner_can_create_role_by_setting_its_owner() {
    let mut contract = new_contract();
    let view = call_as(OWNER_ID, 0, || {
        contract.set_role_owner(
            "auditor".to_owned(),
            Some(account("auditor-owner.test.near")),
        )
    });
    assert_eq!(view.salary, U128(0));
    assert_eq!(view.owner, account("auditor-owner.test.near"));
    assert_eq!(contract.role_count(), 1);
}

#[test]
fn mint_role_creates_membership() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 100);

    let minted_token_id = call_as(OWNER_ID, 7, || {
        contract.mint_role(user(), "admin".to_owned())
    });

    assert_eq!(minted_token_id, token_id(role_id("admin")));
    assert!(contract.has_role(user(), "admin".to_owned()));
    assert!(contract.has_role_id(user(), U64(role_id("admin"))));
    assert!(!contract.has_role(account("bob.test.near"), "admin".to_owned()));
    assert_eq!(
        contract.members_of("admin".to_owned(), None, None),
        vec![user()]
    );
    assert_eq!(contract.roles_of(user()), vec!["admin".to_owned()]);
    assert_eq!(contract.role_ids_of(user()), vec![U64(role_id("admin"))]);
    assert_eq!(contract.get_role("admin".to_owned()).unwrap().holders, 1);
    assert_eq!(
        contract.last_claim(user(), "admin".to_owned()),
        Some(U64(7))
    );
}

#[test]
fn role_owner_can_mint_and_burn() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    call_as(OWNER_ID, 0, || {
        contract.set_role_owner("admin".to_owned(), Some(account("hr.test.near")))
    });

    call_as("hr.test.near", 0, || {
        contract.mint_role(user(), "admin".to_owned())
    });
    assert!(contract.has_role(user(), "admin".to_owned()));

    let _ = call_as("hr.test.near", 0, || {
        contract.burn_role(user(), "admin".to_owned(), None)
    });
    assert!(!contract.has_role(user(), "admin".to_owned()));
}

#[test]
#[should_panic(expected = "role is not configured")]
fn cannot_mint_unconfigured_role() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.mint_role(user(), "admin".to_owned())
    });
}

#[test]
#[should_panic(expected = "only the contract owner or role owner")]
fn unrelated_account_cannot_mint() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    call_as("mallory.test.near", 0, || {
        contract.mint_role(user(), "admin".to_owned())
    });
}

#[test]
#[should_panic(expected = "account already has this role")]
fn cannot_mint_the_same_role_twice() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    call_as(OWNER_ID, 0, || {
        contract.mint_role(user(), "admin".to_owned());
        contract.mint_role(user(), "admin".to_owned());
    });
}

#[test]
#[should_panic(expected = "maximum number of roles")]
fn enforces_max_roles_per_account() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        for index in 0..=MAX_ROLES_PER_ACCOUNT {
            let role = format!("role-{index}");
            contract.set_role_salary(role.clone(), U128(1));
            contract.mint_role(user(), role);
        }
    });
}

#[test]
fn can_hold_the_maximum_number_of_roles() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        for index in 0..MAX_ROLES_PER_ACCOUNT {
            let role = format!("role-{index}");
            contract.set_role_salary(role.clone(), U128(1));
            contract.mint_role(user(), role);
        }
    });
    assert_eq!(contract.role_ids_of(user()).len(), MAX_ROLES_PER_ACCOUNT);
    assert_eq!(contract.roles_of(user()).len(), MAX_ROLES_PER_ACCOUNT);
}

#[test]
fn burn_role_removes_membership_and_views() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    set_env(owner(), 0);
    contract.mint_role(user(), "admin".to_owned());

    set_env(owner(), 0);
    let _ = contract.burn_role(user(), "admin".to_owned(), None);

    assert!(!contract.has_role(user(), "admin".to_owned()));
    assert!(contract.roles_of(user()).is_empty());
    assert!(contract.role_ids_of(user()).is_empty());
    assert!(contract
        .members_of("admin".to_owned(), None, None)
        .is_empty());
    assert_eq!(contract.last_claim(user(), "admin".to_owned()), None);
    assert_eq!(contract.get_role("admin".to_owned()).unwrap().holders, 0);
    assert_eq!(contract.role_count(), 1, "configuration is retained");
}

#[test]
#[should_panic(expected = "account does not have this role")]
fn cannot_burn_role_the_account_does_not_hold() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    let _ = call_as(OWNER_ID, 0, || {
        contract.burn_role(user(), "admin".to_owned(), None)
    });
}

#[test]
#[should_panic(expected = "role is not configured")]
fn cannot_burn_unconfigured_role() {
    let mut contract = new_contract();
    let _ = call_as(OWNER_ID, 0, || {
        contract.burn_role(user(), "admin".to_owned(), None)
    });
}

#[test]
#[should_panic(expected = "only the contract owner or role owner")]
fn unrelated_account_cannot_burn() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    let _ = call_as("mallory.test.near", 0, || {
        contract.burn_role(user(), "admin".to_owned(), None)
    });
}

#[test]
fn roles_of_and_members_of_are_sorted_and_deduplicated() {
    let mut contract = new_contract();
    create_role(&mut contract, "zeta", 0);
    create_role(&mut contract, "alpha", 0);
    set_env(owner(), 0);
    contract.mint_role(user(), "zeta".to_owned());
    contract.mint_role(user(), "alpha".to_owned());
    contract.mint_role(account("bob.test.near"), "zeta".to_owned());

    assert_eq!(
        contract.roles_of(user()),
        vec!["alpha".to_owned(), "zeta".to_owned()]
    );
    assert_eq!(contract.role_ids_of(user()), {
        let mut ids = vec![role_id("alpha"), role_id("zeta")];
        ids.sort_unstable();
        ids.into_iter().map(U64).collect::<Vec<_>>()
    });
    assert_eq!(
        contract.members_of("zeta".to_owned(), None, None),
        vec![user(), account("bob.test.near")]
    );
    assert!(contract.roles_of(account("nobody.test.near")).is_empty());
    assert!(contract
        .members_of("unknown".to_owned(), None, None)
        .is_empty());
}

#[test]
fn roles_are_paginated() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        for role in ["one", "two", "three"] {
            contract.set_role_salary(role.to_owned(), U128(1));
        }
    });

    let page = contract.get_roles(Some(U128(1)), Some(1));
    assert_eq!(page.len(), 1);
    assert!(page[0].name == "two" || page[0].name == "three");
    assert_eq!(contract.get_roles(Some(U128(10)), Some(1)).len(), 0);
    assert_eq!(contract.get_roles(None, None).len(), 3);
}

#[test]
fn set_payout_config_updates_payroll_settings() {
    let mut contract = new_contract();
    let new_token = account("usdc.test.near");

    call_as(OWNER_ID, 0, || {
        contract.set_payout_config(new_token.clone(), U64(60_000_000_000))
    });

    assert_eq!(contract.salary_token(), new_token);
    assert_eq!(contract.payout_interval_ns(), U64(60_000_000_000));
}

#[test]
#[should_panic(expected = "payout interval must be positive")]
fn set_payout_config_rejects_zero_interval() {
    let mut contract = new_contract();
    call_as(OWNER_ID, 0, || {
        contract.set_payout_config(account("usdc.test.near"), U64(0))
    });
}

#[test]
#[should_panic(expected = "only the contract owner")]
fn non_owner_cannot_change_payout_config() {
    let mut contract = new_contract();
    call_as("mallory.test.near", 0, || {
        contract.set_payout_config(account("usdc.test.near"), U64(1))
    });
}

#[test]
fn mint_and_burn_emit_nep_297_events() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    set_env(owner(), 0);
    contract.mint_role(user(), "admin".to_owned());

    let logs = get_logs();
    let mint = logs
        .iter()
        .find(|log| log.contains("\"event\":\"mt_mint\""))
        .expect("mint event");
    assert!(mint.starts_with("EVENT_JSON:"));
    let json: serde_json::Value =
        serde_json::from_str(mint.trim_start_matches("EVENT_JSON:")).unwrap();
    assert_eq!(json["standard"], "nep245");
    assert_eq!(json["version"], "1.0.0");
    assert_eq!(json["data"][0]["owner_id"], user().to_string());
    assert_eq!(json["data"][0]["token_ids"][0], token_id(role_id("admin")));
    assert_eq!(json["data"][0]["amounts"][0], "1");

    set_env(owner(), 0);
    let _ = contract.burn_role(user(), "admin".to_owned(), None);
    let logs = get_logs();
    let burn = logs
        .iter()
        .find(|log| log.contains("\"event\":\"mt_burn\""))
        .expect("burn event");
    let json: serde_json::Value =
        serde_json::from_str(burn.trim_start_matches("EVENT_JSON:")).unwrap();
    assert_eq!(json["data"][0]["owner_id"], user().to_string());
    assert_eq!(json["data"][0]["amounts"][0], "1");
    assert_eq!(
        json["data"][0]["authorized_id"], OWNER_ID,
        "the contract owner burned someone else's token"
    );
}

#[test]
fn burn_event_records_the_authorized_caller() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    set_env(owner(), 0);
    contract.mint_role(user(), "admin".to_owned());
    let _ = call_as(OWNER_ID, 0, || {
        contract.burn_role(user(), "admin".to_owned(), None)
    });

    let logs = get_logs();
    let burn = logs
        .iter()
        .find(|log| log.contains("\"event\":\"mt_burn\""))
        .expect("burn event");
    let json: serde_json::Value =
        serde_json::from_str(burn.trim_start_matches("EVENT_JSON:")).unwrap();
    assert_eq!(
        json["data"][0]["authorized_id"], OWNER_ID,
        "burning someone else's token records the caller"
    );
}

#[test]
fn mint_requires_the_storage_deposit() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    let required = contract.required_mint_deposit().0;

    set_env_with_deposit(OWNER_ID, 0, NearToken::from_yoctonear(required - 1));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        contract.mint_role(user(), "admin".to_owned());
    }));
    assert!(result.is_err(), "minting below the reserve should fail");

    set_env_with_deposit(OWNER_ID, 0, NearToken::from_yoctonear(required));
    contract.mint_role(user(), "admin".to_owned());
    assert!(contract.has_role(user(), "admin".to_owned()));
}

#[test]
fn members_are_paginated_and_swap_removal_keeps_the_set_consistent() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    let bob = account("bob.test.near");
    let carol = account("carol.test.near");
    set_env(owner(), 0);
    contract.mint_role(user(), "admin".to_owned());
    contract.mint_role(bob.clone(), "admin".to_owned());
    contract.mint_role(carol.clone(), "admin".to_owned());

    assert_eq!(
        contract.members_of("admin".to_owned(), None, None),
        vec![user(), bob.clone(), carol.clone()]
    );
    assert_eq!(
        contract.members_of("admin".to_owned(), Some(U128(1)), Some(1)),
        vec![bob.clone()]
    );
    assert!(contract
        .members_of("admin".to_owned(), Some(U128(99)), None)
        .is_empty());

    set_env(owner(), 0);
    let _ = contract.burn_role(bob, "admin".to_owned(), Some(false));
    assert_eq!(
        contract.members_of("admin".to_owned(), None, None),
        vec![user(), carol]
    );
    assert_eq!(contract.get_role("admin".to_owned()).unwrap().holders, 2);
}

#[test]
fn factory_deployed_contract_notifies_the_factory_on_owner_change() {
    let factory = account("factory.test.near");
    let mut contract = new_contract_with_factory(Some(factory.clone()));
    assert_eq!(contract.factory_id(), Some(factory.clone()));

    let new_owner = account("new-dao.test.near");
    set_env(owner(), 0);
    match contract.set_owner(new_owner.clone()) {
        PromiseOrValue::Promise(_) => {}
        PromiseOrValue::Value(_) => panic!("expected a factory notification"),
    }

    let receipts = get_receipts();
    let receipt = receipts
        .iter()
        .find(|receipt| receipt.receiver_id == factory)
        .expect("factory notification receipt");
    assert_eq!(
        receipt_methods(receipt),
        vec!["on_owner_changed".to_owned()]
    );
    let (_, args) = receipt_function_call(receipt).unwrap();
    let args: serde_json::Value = serde_json::from_slice(&args).unwrap();
    assert_eq!(args["new_owner_id"], new_owner.to_string());
}

#[test]
fn directly_deployed_contract_updates_ownership_without_a_factory_call() {
    let mut contract = new_contract();
    let new_owner = account("new-dao.test.near");
    set_env(owner(), 0);
    match contract.set_owner(new_owner.clone()) {
        PromiseOrValue::Value(ok) => assert!(ok),
        PromiseOrValue::Promise(_) => panic!("direct deployments have no factory to notify"),
    }
    assert_eq!(contract.owner_id(), new_owner);
    assert!(get_receipts().is_empty());
}
