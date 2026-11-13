//! Minimal NEP-141 token for sandbox tests.
//!
//! It supports only the surface the RBAC contract needs: balances, metadata, `ft_transfer`, and a
//! switch that makes transfers fail so the callback rollback paths can be exercised.

use near_sdk::json_types::U128;
use near_sdk::store::LookupMap;
use near_sdk::{env, near, require, AccountId, NearToken, PanicOnDefault, Promise};

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct MockFt {
    balances: LookupMap<AccountId, u128>,
    fail_transfers: bool,
}

#[near]
impl MockFt {
    /// `seed` is required so deploying this contract with the RBAC init args fails on purpose.
    #[init]
    pub fn new(seed: u64) -> Self {
        require!(seed > 0, "seed must be positive");
        Self {
            balances: LookupMap::new(b"balances".as_slice()),
            fail_transfers: false,
        }
    }

    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        let current = self.balances.get(&account_id).copied().unwrap_or(0);
        self.balances
            .insert(account_id, current.saturating_add(amount.0));
    }

    pub fn set_fail_transfers(&mut self, fail: bool) {
        self.fail_transfers = fail;
    }

    pub fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.balances.get(&account_id).copied().unwrap_or(0))
    }

    pub fn ft_total_supply(&self) -> U128 {
        U128(0)
    }

    pub fn ft_metadata(&self) -> String {
        "{\"spec\":\"ft-1.0.0\",\"name\":\"Mock Token\",\"symbol\":\"MOCK\",\"decimals\":6}"
            .to_owned()
    }

    #[payable]
    pub fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        let _ = memo;
        require!(
            env::attached_deposit() >= NearToken::from_yoctonear(1),
            "attached deposit must be at least 1 yoctoNEAR"
        );
        if self.fail_transfers {
            env::panic_str("mock transfers are disabled");
        }
        let sender = env::predecessor_account_id();
        let balance = self.balances.get(&sender).copied().unwrap_or(0);
        require!(balance >= amount.0, "insufficient balance");
        self.balances.insert(sender, balance - amount.0);
        let receiver_balance = self.balances.get(&receiver_id).copied().unwrap_or(0);
        self.balances
            .insert(receiver_id, receiver_balance.saturating_add(amount.0));
    }

    #[allow(dead_code)]
    pub fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> Promise {
        let _ = (receiver_id, amount, memo, msg);
        env::panic_str("not implemented")
    }
}
