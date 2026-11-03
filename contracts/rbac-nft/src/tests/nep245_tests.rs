//! Tests for the NEP-245 core, enumeration and metadata surfaces.

use near_sdk::json_types::{U128, U64};
use rbac_common::{base_metadata_id, role_id, token_id};

use super::helpers::*;

#[test]
fn token_and_balance_views() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);

    let id = token_id("admin", "alice.test.near");
    assert_eq!(contract.mt_balance_of(user(), id.clone()), U128(1));
    assert_eq!(
        contract.mt_balance_of(account("bob.test.near"), id.clone()),
        U128(0)
    );
    assert_eq!(contract.mt_supply(id.clone()), Some(U128(1)));
    assert_eq!(contract.mt_supply("missing".to_owned()), None);

    let tokens = contract.mt_token(vec![id.clone(), "missing".to_owned()]);
    assert_eq!(tokens.len(), 2);
    let token = tokens[0].as_ref().expect("existing token");
    assert_eq!(token.token_id, id);
    assert_eq!(token.owner_id, Some(user()));
    assert_eq!(token.base_metadata_id, base_metadata_id(role_id("admin")));
    assert_eq!(token.token_metadata, None);
    assert!(tokens[1].is_none());
}

#[test]
fn batch_balance_and_supply() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 0, 0);

    let admin = token_id("admin", "alice.test.near");
    let employee = token_id("employee", "alice.test.near");
    let missing = token_id("manager", "alice.test.near");

    assert_eq!(
        contract.mt_batch_balance_of(
            user(),
            vec![admin.clone(), employee.clone(), missing.clone()]
        ),
        vec![U128(1), U128(1), U128(0)]
    );
    assert_eq!(
        contract.mt_batch_supply(vec![admin, employee, missing]),
        vec![Some(U128(1)), Some(U128(1)), None]
    );
}

#[test]
fn token_supply_is_scoped_to_the_exact_account() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);

    assert_eq!(contract.mt_supply(token_id("admin", "bob.test.near")), None);
    assert_eq!(
        contract.mt_balance_of(
            account("bob.test.near"),
            token_id("admin", "alice.test.near")
        ),
        U128(0)
    );
}

#[test]
fn enumerates_tokens_and_tokens_for_owner() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 0, 0);
    grant_role(&mut contract, "alice.test.near", "manager", 0, 0);
    grant_role(&mut contract, "bob.test.near", "employee", 0, 0);

    assert_eq!(contract.mt_tokens(None, None).len(), 4);
    assert_eq!(contract.mt_tokens(Some(U128(1)), Some(2)).len(), 2);
    assert_eq!(contract.mt_tokens(Some(U128(10)), None).len(), 0);

    let alice_tokens = contract.mt_tokens_for_owner(user(), None, None);
    assert_eq!(alice_tokens.len(), 3);
    assert!(alice_tokens
        .iter()
        .all(|token| token.owner_id == Some(user())));

    let page = contract.mt_tokens_for_owner(user(), Some(U128(1)), Some(1));
    assert_eq!(page.len(), 1);

    assert!(contract
        .mt_tokens_for_owner(account("nobody.test.near"), None, None)
        .is_empty());
}

#[test]
fn traversal_order_survives_burns() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    grant_role(&mut contract, "alice.test.near", "employee", 0, 0);

    set_env(owner(), 0);
    contract.burn_role(user(), "admin".to_owned());

    let tokens = contract.mt_tokens(None, None);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].token_id, token_id("employee", "alice.test.near"));
}

#[test]
fn contract_metadata() {
    let contract = new_contract();
    let metadata = contract.mt_metadata_contract();
    assert_eq!(metadata.spec, "mt-1.0.0");
    assert_eq!(metadata.name, "NEAR RBAC Role NFTs");
}

#[test]
fn base_metadata_by_id_and_token_id() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 1_234, 0);

    let id = base_metadata_id(role_id("admin"));
    let base = contract.mt_metadata_base_by_metadata_id(vec![id.clone()]);
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].name, "admin");
    assert_eq!(base[0].id, id);
    assert_eq!(base[0].decimals.as_deref(), Some("0"));
    assert_eq!(base[0].copies, Some(1));
    assert_eq!(base[0].symbol, None);

    let by_token =
        contract.mt_metadata_base_by_token_id(vec![token_id("admin", "alice.test.near")]);
    assert_eq!(by_token, base);
}

#[test]
fn base_metadata_for_a_role_without_members() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    let base = contract.mt_metadata_base_by_metadata_id(vec![base_metadata_id(role_id("admin"))]);
    assert_eq!(base[0].copies, Some(0));
}

#[test]
#[should_panic(expected = "invalid base metadata id")]
fn base_metadata_rejects_non_numeric_ids() {
    let contract = new_contract();
    contract.mt_metadata_base_by_metadata_id(vec!["admin".to_owned()]);
}

#[test]
#[should_panic(expected = "unknown role id")]
fn base_metadata_rejects_unknown_role_ids() {
    let contract = new_contract();
    contract.mt_metadata_base_by_metadata_id(vec!["42".to_owned()]);
}

#[test]
#[should_panic(expected = "unknown token id")]
fn base_metadata_rejects_unknown_token_ids() {
    let contract = new_contract();
    contract.mt_metadata_base_by_token_id(vec!["1:alice.test.near".to_owned()]);
}

#[test]
fn token_metadata_describes_membership() {
    let mut contract = new_contract();
    grant_role(
        &mut contract,
        "alice.test.near",
        "admin",
        500,
        2_000_000_000,
    );
    let id = token_id("admin", "alice.test.near");

    let all = contract.mt_metadata_token_all(vec![id.clone()]);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].base.name, "admin");
    assert_eq!(all[0].token.title.as_deref(), Some("admin role"));
    assert!(all[0]
        .token
        .description
        .as_deref()
        .unwrap()
        .contains("alice.test.near"));
    assert_eq!(all[0].token.issued_at.as_deref(), Some("2000"));

    let extra: serde_json::Value =
        serde_json::from_str(all[0].token.extra.as_deref().unwrap()).unwrap();
    assert_eq!(extra["role"], "admin");
    assert_eq!(extra["role_id"], role_id("admin"));
    assert_eq!(extra["salary"], "500");
    assert_eq!(extra["payout_interval_ns"], INTERVAL_NS.to_string());

    let instance = contract.mt_metadata_token_by_token_id(vec![id]);
    assert_eq!(instance.len(), 1);
    assert_eq!(instance[0], all[0].token);
}

#[test]
#[should_panic(expected = "unknown token id")]
fn token_metadata_rejects_unknown_token_ids() {
    let contract = new_contract();
    contract.mt_metadata_token_all(vec!["1:alice.test.near".to_owned()]);
}

#[test]
fn all_base_metadata_is_paginated() {
    let mut contract = new_contract();
    create_role(&mut contract, "admin", 0);
    create_role(&mut contract, "employee", 0);
    create_role(&mut contract, "manager", 0);

    let all = contract.mt_tokens_base_metadata_all(None, None);
    assert_eq!(all.len(), 3);
    assert_eq!(
        contract
            .mt_tokens_base_metadata_all(Some(U128(1)), Some(1))
            .len(),
        1
    );
    assert!(contract
        .mt_tokens_base_metadata_all(Some(U128(10)), None)
        .is_empty());

    let ids: Vec<String> = all.into_iter().map(|base| base.id).collect();
    assert!(ids.contains(&base_metadata_id(role_id("admin"))));
    assert!(ids.contains(&base_metadata_id(role_id("employee"))));
    assert!(ids.contains(&base_metadata_id(role_id("manager"))));
}

#[test]
fn metadata_reflects_salary_updates() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    call_as(OWNER_ID, 0, || {
        contract.set_role_salary("admin".to_owned(), U128(900))
    });

    let all = contract.mt_metadata_token_all(vec![token_id("admin", "alice.test.near")]);
    let extra: serde_json::Value =
        serde_json::from_str(all[0].token.extra.as_deref().unwrap()).unwrap();
    assert_eq!(extra["salary"], "900");
}

#[test]
fn metadata_reflects_payout_interval_updates() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 100, 0);
    call_as(OWNER_ID, 0, || {
        contract.set_payout_config(account("usdc.test.near"), U64(2_000_000_000))
    });

    let all = contract.mt_metadata_token_all(vec![token_id("admin", "alice.test.near")]);
    let extra: serde_json::Value =
        serde_json::from_str(all[0].token.extra.as_deref().unwrap()).unwrap();
    assert_eq!(extra["payout_interval_ns"], "2000000000");
}

#[test]
#[should_panic(expected = "role NFTs are non-transferable")]
fn mt_transfer_is_disabled() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    set_env(owner(), 0);
    contract.mt_transfer(
        account("bob.test.near"),
        token_id("admin", "alice.test.near"),
        U128(1),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "role NFTs are non-transferable")]
fn mt_batch_transfer_is_disabled() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    set_env(owner(), 0);
    contract.mt_batch_transfer(
        account("bob.test.near"),
        vec![token_id("admin", "alice.test.near")],
        vec![U128(1)],
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "role NFTs are non-transferable")]
fn mt_transfer_call_is_disabled() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    set_env(owner(), 0);
    let _ = contract.mt_transfer_call(
        account("bob.test.near"),
        token_id("admin", "alice.test.near"),
        U128(1),
        None,
        None,
        "{}".to_owned(),
    );
}

#[test]
#[should_panic(expected = "role NFTs are non-transferable")]
fn mt_batch_transfer_call_is_disabled() {
    let mut contract = new_contract();
    grant_role(&mut contract, "alice.test.near", "admin", 0, 0);
    set_env(owner(), 0);
    let _ = contract.mt_batch_transfer_call(
        account("bob.test.near"),
        vec![token_id("admin", "alice.test.near")],
        vec![U128(1)],
        None,
        None,
        "{}".to_owned(),
    );
}
