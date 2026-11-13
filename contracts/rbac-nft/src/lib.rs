#![doc = include_str!("../README.md")]

mod events;
mod metadata;

#[cfg(test)]
mod tests;

use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::json_types::{U128, U64};
use near_sdk::store::{IterableMap, LookupMap};
use near_sdk::{
    env, ext_contract, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise,
    PromiseOrValue,
};
use rbac_common::{base_metadata_id, parse_token_id, role_id, token_id, validate_role_name};

use crate::metadata::{
    MTBaseTokenMetadata, MTContractMetadata, MTRoleToken, MTTokenMetadata, MTTokenMetadataAll,
};

/// Maximum number of role memberships one account may hold.
///
/// `claim` iterates a member's roles in one call. Capping the number keeps claims well inside the
/// block gas limit and makes salary payout predictable.
///
/// This is a build-time constant, not a runtime setting: change it and redeploy the contract.
pub const MAX_ROLES_PER_ACCOUNT: usize = 32;

/// Bytes charged to a minter for one role membership.
///
/// `mint_role` is payable so that role owners cannot drain the contract's NEAR balance by minting
/// memberships. The deposit covers the new holder entry and related bookkeeping. It is not
/// refunded when the role is burned, because the storage is not reclaimed immediately.
pub const MINT_STORAGE_RESERVE_BYTES: u128 = 1_024;

const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 100;

/// A configured role.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Role {
    /// Human-readable role name. It is case-sensitive and hashed byte-for-byte.
    pub name: String,
    /// Optional role owner. `None` means the contract owner is the effective role owner.
    pub owner: Option<AccountId>,
    /// Salary paid per full payout interval, in the smallest unit of the salary token.
    pub salary: u128,
}

impl Role {
    fn effective_owner(&self, contract_owner: &AccountId) -> AccountId {
        self.owner.clone().unwrap_or_else(|| contract_owner.clone())
    }
}

/// Public view of a role.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleView {
    pub role_id: U64,
    pub name: String,
    /// Effective owner (falls back to the contract owner).
    pub owner: AccountId,
    /// Explicitly configured owner, if any.
    pub configured_owner: Option<AccountId>,
    pub salary: U128,
    pub holders: u64,
}

#[allow(dead_code)]
#[ext_contract(ext_ft)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[allow(dead_code)]
#[ext_contract(ext_factory)]
trait Factory {
    fn on_owner_changed(&mut self, new_owner_id: AccountId);
}

/// Non-transferable NEP-245 role NFT and salary contract.
#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    /// DAO, Trezu, or other account that owns this extension.
    owner_id: AccountId,
    /// NEP-141 token used for salary payouts.
    salary_token: AccountId,
    /// Global payout interval in nanoseconds.
    payout_interval_ns: u64,
    /// Factory that deployed this contract, if any. Used to keep the factory's owner index fresh.
    factory_id: Option<AccountId>,

    /// role id -> role configuration. Iterable for metadata/enumeration.
    roles: IterableMap<u64, Role>,
    /// role name -> role id, used for collision detection and lookups.
    role_by_name: LookupMap<String, u64>,
    /// account -> role ids. Kept flat because the number of roles per account is capped.
    owned_tokens: LookupMap<AccountId, Vec<u64>>,
    /// (role id, holder index) -> member account. Sparse indexed storage avoids rewriting a Vec.
    role_holders: LookupMap<(u64, u32), AccountId>,
    /// (role id, account) -> holder index, used for O(1) removal.
    holder_index: LookupMap<(u64, AccountId), u32>,
    /// role id -> number of holders.
    holder_count: LookupMap<u64, u32>,
    /// (role id, account) -> last salary claim timestamp in nanoseconds.
    last_claim: LookupMap<(u64, AccountId), u64>,
}

#[near]
impl Contract {
    /// Creates the extension.
    ///
    /// `owner_id` should normally be the Sputnik DAO or Trezu account that governs this
    /// extension. `salary_token` is the NEP-141 token the contract pays salaries in, and
    /// `payout_interval_ns` is the global payout period (`1_000_000_000` = one second,
    /// `2_592_000_000_000_000` = 30 days).
    ///
    /// `factory_id` is set by the factory to its own account id. The contract notifies it when
    /// ownership changes so the factory's owner index stays current. Direct deployments should
    /// pass `null`.
    #[init]
    pub fn new(
        owner_id: AccountId,
        salary_token: AccountId,
        payout_interval_ns: U64,
        factory_id: Option<AccountId>,
    ) -> Self {
        require!(payout_interval_ns.0 > 0, "payout interval must be positive");
        Self {
            owner_id,
            salary_token,
            payout_interval_ns: payout_interval_ns.0,
            factory_id,
            roles: IterableMap::new(b"roles".as_slice()),
            role_by_name: LookupMap::new(b"role_names".as_slice()),
            owned_tokens: LookupMap::new(b"owned_tokens".as_slice()),
            role_holders: LookupMap::new(b"role_holders".as_slice()),
            holder_index: LookupMap::new(b"holder_index".as_slice()),
            holder_count: LookupMap::new(b"holder_count".as_slice()),
            last_claim: LookupMap::new(b"last_claim".as_slice()),
        }
    }

    // ---------------------------------------------------------------------
    // Configuration
    // ---------------------------------------------------------------------

    /// Returns the contract owner (normally the DAO).
    pub fn owner_id(&self) -> AccountId {
        self.owner_id.clone()
    }

    /// Returns the NEP-141 salary token account.
    pub fn salary_token(&self) -> AccountId {
        self.salary_token.clone()
    }

    /// Returns the global salary payout interval in nanoseconds.
    pub fn payout_interval_ns(&self) -> U64 {
        U64(self.payout_interval_ns)
    }

    /// Returns the factory that deployed this contract, if any.
    pub fn factory_id(&self) -> Option<AccountId> {
        self.factory_id.clone()
    }

    /// Returns the NEAR deposit required to mint one role membership at the current storage price.
    pub fn required_mint_deposit(&self) -> U128 {
        U128(
            env::storage_byte_cost()
                .as_yoctonear()
                .saturating_mul(MINT_STORAGE_RESERVE_BYTES),
        )
    }

    /// Transfers ownership of the extension. Only the current owner may call this.
    ///
    /// If the contract was deployed by a factory, it best-effort notifies the factory so the
    /// factory's owner index can be updated. The ownership change succeeds even when that
    /// notification fails; clients should always verify `owner_id()` on the child.
    pub fn set_owner(&mut self, new_owner_id: AccountId) -> PromiseOrValue<bool> {
        self.assert_owner();
        self.owner_id = new_owner_id.clone();
        if let Some(factory_id) = self.factory_id.clone() {
            PromiseOrValue::Promise(
                ext_factory::ext(factory_id)
                    .with_static_gas(Gas::from_tgas(15))
                    .on_owner_changed(new_owner_id),
            )
        } else {
            PromiseOrValue::Value(true)
        }
    }

    /// Updates the salary token and/or payout interval. Only the contract owner may call this.
    ///
    /// Existing unclaimed salary is calculated with the new configuration for the whole elapsed
    /// period. Callers should claim before changing payroll configuration if that matters.
    pub fn set_payout_config(&mut self, salary_token: AccountId, payout_interval_ns: U64) {
        self.assert_owner();
        require!(payout_interval_ns.0 > 0, "payout interval must be positive");
        self.salary_token = salary_token;
        self.payout_interval_ns = payout_interval_ns.0;
    }

    /// Creates a role or updates its salary. Only the contract owner may call this.
    ///
    /// Role names are hashed to a 64-bit id. If the id is already used by a different role name,
    /// this panics rather than silently merging the roles.
    ///
    /// Salary changes apply retroactively to unclaimed time; claim before changing a salary if
    /// that is not desired.
    pub fn set_role_salary(&mut self, role: String, salary: U128) -> RoleView {
        self.assert_owner();
        let role_id = self.get_or_create_role_id(&role);
        let mut config = self.roles.get(&role_id).cloned().unwrap_or(Role {
            name: role,
            owner: None,
            salary: 0,
        });
        config.salary = salary.0;
        let view = self.role_view(role_id, &config);
        self.roles.insert(role_id, config);
        view
    }

    /// Sets or clears the owner of a role.
    ///
    /// The contract owner may always do this. The current effective role owner may hand the role
    /// to another account (or clear the explicit owner, falling back to the contract owner).
    /// Only the contract owner can set a role owner if the role does not exist yet.
    pub fn set_role_owner(&mut self, role: String, owner: Option<AccountId>) -> RoleView {
        let role_id = self.get_or_create_role_id(&role);
        let mut config = self.roles.get(&role_id).cloned().unwrap_or(Role {
            name: role,
            owner: None,
            salary: 0,
        });
        let caller = env::predecessor_account_id();
        let effective = config.effective_owner(&self.owner_id);
        require!(
            caller == self.owner_id || caller == effective,
            "only the contract owner or role owner can set the role owner"
        );
        config.owner = owner;
        let view = self.role_view(role_id, &config);
        self.roles.insert(role_id, config);
        view
    }

    /// Returns a role by name, if configured.
    pub fn get_role(&self, role: String) -> Option<RoleView> {
        let role_id = role_id(&role);
        self.roles
            .get(&role_id)
            .map(|config| self.role_view(role_id, config))
    }

    /// Returns a role by its 64-bit role id, if configured.
    pub fn get_role_by_id(&self, role_id: U64) -> Option<RoleView> {
        self.roles
            .get(&role_id.0)
            .map(|config| self.role_view(role_id.0, config))
    }

    /// Lists configured roles with pagination.
    pub fn get_roles(&self, from_index: Option<U128>, limit: Option<u32>) -> Vec<RoleView> {
        let (from, limit) = pagination(from_index, limit);
        self.roles
            .iter()
            .skip(from)
            .take(limit)
            .map(|(role_id, config)| self.role_view(*role_id, config))
            .collect()
    }

    /// Number of configured roles.
    pub fn role_count(&self) -> u32 {
        self.roles.len()
    }

    // ---------------------------------------------------------------------
    // Role membership (NEP-245 mint/burn)
    // ---------------------------------------------------------------------

    /// Mints a role NFT to `account_id`.
    ///
    /// Callable by the contract owner or the role owner. An account can hold each role at most
    /// once. The token id is the decimal role id, and the balance for that token type is `1`.
    /// Minting starts salary accrual at the current block timestamp.
    ///
    /// The call must attach at least [`Contract::required_mint_deposit`]; the deposit pays for the
    /// new membership's storage and is not refunded when the role is burned.
    #[payable]
    pub fn mint_role(&mut self, account_id: AccountId, role: String) -> String {
        let role_id = role_id(&role);
        let config = self
            .roles
            .get(&role_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("role is not configured; set its salary first"));
        require!(
            config.name == role,
            "role name hash collision; choose a different role name"
        );
        self.assert_can_manage_role(&config);

        require!(
            !self.is_member(&account_id, role_id),
            "account already has this role"
        );

        let mut owned = self
            .owned_tokens
            .get(&account_id)
            .cloned()
            .unwrap_or_default();
        require!(
            owned.len() < MAX_ROLES_PER_ACCOUNT,
            "account has reached the maximum number of roles"
        );

        let required = self.required_mint_deposit().0;
        require!(
            env::attached_deposit().as_yoctonear() >= required,
            "attached deposit is too low; call required_mint_deposit"
        );

        owned.push(role_id);
        self.owned_tokens.insert(account_id.clone(), owned);

        self.add_holder(role_id, &account_id);

        let now = env::block_timestamp();
        self.last_claim.insert((role_id, account_id.clone()), now);

        let token_id = token_id(role_id);
        events::emit_mint(&account_id, &token_id);
        token_id
    }

    /// Burns an account's role NFT.
    ///
    /// Callable by the contract owner or the role owner. By default, any accrued salary for this
    /// role is transferred to the member before the membership is removed. Pass
    /// `pay_pending: false` to forfeit accrued salary instead.
    ///
    /// When pay is due, the transfer is asynchronous: if it fails, the membership is kept and the
    /// claim timestamp is restored, so a failed payout never silently discards salary.
    pub fn burn_role(
        &mut self,
        account_id: AccountId,
        role: String,
        pay_pending: Option<bool>,
    ) -> PromiseOrValue<bool> {
        let role_id = role_id(&role);
        let config = self
            .roles
            .get(&role_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("role is not configured"));
        require!(
            config.name == role,
            "role name hash collision; choose a different role name"
        );
        self.assert_can_manage_role(&config);
        require!(
            self.is_member(&account_id, role_id),
            "account does not have this role"
        );

        let caller = env::predecessor_account_id();
        let authorized_id = (caller != account_id).then_some(caller);
        let amount = self.claimable_role_amount(&account_id, role_id);
        let pay_pending = pay_pending.unwrap_or(true);

        if !pay_pending || amount == 0 {
            self.remove_member(&account_id, role_id, authorized_id);
            return PromiseOrValue::Value(true);
        }

        let key = (role_id, account_id.clone());
        let now = env::block_timestamp();
        let interval = self.payout_interval_ns;
        let last = self.last_claim.get(&key).copied().unwrap_or(now);
        let periods = now.saturating_sub(last) / interval;
        let advance_ns = periods
            .checked_mul(interval)
            .unwrap_or_else(|| env::panic_str("claim timestamp overflow"));
        let next = last
            .checked_add(advance_ns)
            .unwrap_or_else(|| env::panic_str("claim timestamp overflow"));
        self.last_claim.insert(key, next);

        let promise = ext_ft::ext(self.salary_token.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(30))
            .ft_transfer(account_id.clone(), U128(amount), None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(25))
                    .on_burn_transfer(
                        account_id.clone(),
                        U64(role_id),
                        U64(last),
                        U128(amount),
                        authorized_id,
                    ),
            );
        PromiseOrValue::Promise(promise)
    }

    /// Returns true when `account_id` currently holds `role`.
    ///
    /// This is the off-chain/check helper used by services. On-chain contracts should use the
    /// same token-id check at the start of their own methods (see README) or call this contract
    /// asynchronously before performing a gated action.
    pub fn has_role(&self, account_id: AccountId, role: String) -> bool {
        self.has_role_id(account_id, U64(role_id(&role)))
    }

    /// Returns true when `account_id` holds the role identified by `role_id`.
    ///
    /// `role_id` is the first 8 bytes of `SHA-256(role_name)` as a big-endian `u64`; it is also
    /// the NEP-245 token id and base metadata id of the role.
    pub fn has_role_id(&self, account_id: AccountId, role_id: U64) -> bool {
        self.is_member(&account_id, role_id.0)
    }

    /// Returns the human-readable roles held by an account. Used by the OIDC service.
    pub fn roles_of(&self, account_id: AccountId) -> Vec<String> {
        let mut roles: Vec<String> = self
            .owned_tokens
            .get(&account_id)
            .into_iter()
            .flatten()
            .filter_map(|role_id| self.roles.get(role_id).map(|role| role.name.clone()))
            .collect();
        roles.sort();
        roles.dedup();
        roles
    }

    /// Returns the role ids held by an account.
    pub fn role_ids_of(&self, account_id: AccountId) -> Vec<U64> {
        let mut role_ids: Vec<u64> = self
            .owned_tokens
            .get(&account_id)
            .cloned()
            .unwrap_or_default();
        role_ids.sort_unstable();
        role_ids.into_iter().map(U64).collect()
    }

    /// Returns a page of members of a role.
    ///
    /// Holders are stored in indexed maps, so membership reads are O(limit) rather than O(n).
    /// The order is insertion order except that burning a member swaps the last holder into the
    /// removed slot. Sort off-chain if a stable display order is required.
    pub fn members_of(
        &self,
        role: String,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<AccountId> {
        let role_id = role_id(&role);
        let (from, limit) = pagination(from_index, limit);
        let count = self.holder_count(role_id);
        let from = u32::try_from(from).unwrap_or(u32::MAX).min(count);
        let end = from.saturating_add(limit as u32).min(count);
        (from..end)
            .filter_map(|index| self.role_holders.get(&(role_id, index)).cloned())
            .collect()
    }

    // ---------------------------------------------------------------------
    // Salary
    // ---------------------------------------------------------------------

    /// Claims all salary accrued by `account_id` and transfers it in the salary token.
    ///
    /// Anyone may trigger a claim; the payout always goes to `account_id`. The transaction
    /// returns the claimed amount directly when nothing is due, otherwise it returns the promise
    /// for the NEP-141 transfer. If the transfer fails, the callback restores the previous claim
    /// timestamps. Callers should check the final execution status.
    pub fn claim(&mut self, account_id: AccountId) -> PromiseOrValue<U128> {
        self.claim_internal(&account_id, None)
    }

    /// Claims salary for one role only. Useful when an account holds many roles.
    pub fn claim_role(&mut self, account_id: AccountId, role: String) -> PromiseOrValue<U128> {
        self.claim_internal(&account_id, Some(role_id(&role)))
    }

    /// Returns the total salary currently claimable by `account_id`, without transferring it.
    pub fn claimable(&self, account_id: AccountId) -> U128 {
        U128(self.claimable_for_internal(&account_id, None))
    }

    /// Returns the salary currently claimable for one role, without transferring it.
    pub fn claimable_role(&self, account_id: AccountId, role: String) -> U128 {
        U128(self.claimable_for_internal(&account_id, Some(role_id(&role))))
    }

    /// Returns the last claim timestamp for an account's role, in nanoseconds.
    pub fn last_claim(&self, account_id: AccountId, role: String) -> Option<U64> {
        self.last_claim
            .get(&(role_id(&role), account_id))
            .copied()
            .map(U64)
    }

    /// Returns the total salary currently owed to all members across all roles.
    ///
    /// This is a view for funding checks: compare it with the contract's balance from the token's
    /// `ft_balance_of(contract_id)`. The contract cannot read that balance itself, so the web app
    /// composes both. This view is O(number of memberships); for large deployments use
    /// [`Contract::accrued_liabilities_for_role`] for individual roles.
    pub fn accrued_liabilities(&self) -> U128 {
        let now = env::block_timestamp();
        U128(
            self.roles
                .iter()
                .map(|(role_id, _)| self.role_liability(*role_id, now))
                .fold(0u128, |total, amount| {
                    total
                        .checked_add(amount)
                        .unwrap_or_else(|| env::panic_str("liability overflow"))
                }),
        )
    }

    /// Returns the salary currently owed to members of one role.
    pub fn accrued_liabilities_for_role(&self, role: String) -> U128 {
        U128(self.role_liability(role_id(&role), env::block_timestamp()))
    }

    /// Withdraws NEP-141 tokens from the contract. Only the contract owner may call this.
    ///
    /// `token` is any NEP-141 account, not just the configured salary token, so the owner can
    /// recover balances stranded after `set_payout_config` switches tokens. Attach at least
    /// 1 yoctoNEAR for the `ft_transfer` call.
    ///
    /// This also lets the owner withdraw funds reserved for salaries, so the owner remains a
    /// trusted party. Use [`Contract::accrued_liabilities`] to check funding before withdrawing.
    #[payable]
    pub fn withdraw_ft(
        &mut self,
        token: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> Promise {
        self.assert_owner();
        require!(amount.0 > 0, "amount must be positive");
        require!(
            env::attached_deposit() >= NearToken::from_yoctonear(1),
            "attach at least 1 yoctoNEAR for the token transfer"
        );
        ext_ft::ext(token)
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(30))
            .ft_transfer(receiver_id, amount, None)
    }

    /// Called by the salary token transfer promise. Restores claim timestamps if the transfer
    /// failed. Restricted to self.
    #[private]
    pub fn on_claim_transfer(
        &mut self,
        account_id: AccountId,
        previous_last_claims: Vec<(u64, u64)>,
        _amount: U128,
    ) -> bool {
        require!(
            env::promise_results_count() == 1,
            "expected exactly one promise result"
        );
        if env::promise_result_checked(0, 1024).is_ok() {
            true
        } else {
            for (role_id, previous) in previous_last_claims {
                if self.is_member(&account_id, role_id) {
                    self.last_claim
                        .insert((role_id, account_id.clone()), previous);
                }
            }
            false
        }
    }

    /// Called after the salary transfer that precedes a burn.
    ///
    /// On success the membership is removed. On failure the previous claim timestamp is restored
    /// and the membership is kept, so accrued salary is never silently discarded.
    #[private]
    pub fn on_burn_transfer(
        &mut self,
        account_id: AccountId,
        role_id: U64,
        previous_last_claim: U64,
        _amount: U128,
        authorized_id: Option<AccountId>,
    ) -> bool {
        require!(
            env::promise_results_count() == 1,
            "expected exactly one promise result"
        );
        if env::promise_result_checked(0, 1024).is_ok() {
            if self.is_member(&account_id, role_id.0) {
                self.remove_member(&account_id, role_id.0, authorized_id);
            }
            true
        } else {
            if self.is_member(&account_id, role_id.0) {
                self.last_claim
                    .insert((role_id.0, account_id), previous_last_claim.0);
            }
            false
        }
    }

    // ---------------------------------------------------------------------
    // NEP-245 core (read + non-transferable mutators)
    // ---------------------------------------------------------------------

    /// NEP-245: returns role tokens for the given token ids.
    ///
    /// A token id is the decimal role id. `owner_id` is `None` because a role token has many
    /// holders; use `mt_balance_of` for a specific account.
    pub fn mt_token(&self, token_ids: Vec<String>) -> Vec<Option<MTRoleToken>> {
        token_ids
            .iter()
            .map(|token_id| parse_token_id(token_id).and_then(|role_id| self.view_token(role_id)))
            .collect()
    }

    /// NEP-245: balance of `account_id` for `token_id` (0 or 1).
    pub fn mt_balance_of(&self, account_id: AccountId, token_id: String) -> U128 {
        U128(self.balance_of(&account_id, &token_id))
    }

    /// NEP-245: batch balances.
    pub fn mt_batch_balance_of(&self, account_id: AccountId, token_ids: Vec<String>) -> Vec<U128> {
        token_ids
            .iter()
            .map(|token_id| U128(self.balance_of(&account_id, token_id)))
            .collect()
    }

    /// NEP-245: holder count of a configured role token.
    ///
    /// Returns `None` for an unknown role. A configured role with no holders has supply `0`.
    pub fn mt_supply(&self, token_id: String) -> Option<U128> {
        let role_id = parse_token_id(&token_id)?;
        self.roles.get(&role_id)?;
        Some(U128(self.holder_count(role_id) as u128))
    }

    /// NEP-245: batch supply.
    pub fn mt_batch_supply(&self, token_ids: Vec<String>) -> Vec<Option<U128>> {
        token_ids
            .iter()
            .map(|token_id| self.mt_supply(token_id.clone()))
            .collect()
    }

    /// NEP-245 core transfer method. Role NFTs are deliberately non-transferable.
    #[payable]
    pub fn mt_transfer(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        amount: U128,
        approval: Option<(AccountId, u64)>,
        memo: Option<String>,
    ) -> bool {
        let _ = (receiver_id, token_id, amount, approval, memo);
        env::panic_str("role NFTs are non-transferable")
    }

    /// NEP-245 core batch transfer method. Role NFTs are deliberately non-transferable.
    #[payable]
    pub fn mt_batch_transfer(
        &mut self,
        receiver_id: AccountId,
        token_ids: Vec<String>,
        amounts: Vec<U128>,
        approvals: Option<Vec<Option<(AccountId, u64)>>>,
        memo: Option<String>,
    ) -> Vec<bool> {
        let _ = (receiver_id, token_ids, amounts, approvals, memo);
        env::panic_str("role NFTs are non-transferable")
    }

    /// NEP-245 core transfer-and-call method. Role NFTs are deliberately non-transferable.
    #[payable]
    pub fn mt_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_id: String,
        amount: U128,
        approval: Option<(AccountId, u64)>,
        memo: Option<String>,
        msg: String,
    ) -> Promise {
        let _ = (receiver_id, token_id, amount, approval, memo, msg);
        env::panic_str("role NFTs are non-transferable")
    }

    /// NEP-245 core batch transfer-and-call method. Role NFTs are deliberately non-transferable.
    #[payable]
    pub fn mt_batch_transfer_call(
        &mut self,
        receiver_id: AccountId,
        token_ids: Vec<String>,
        amounts: Vec<U128>,
        approvals: Option<Vec<Option<(AccountId, u64)>>>,
        memo: Option<String>,
        msg: String,
    ) -> Promise {
        let _ = (receiver_id, token_ids, amounts, approvals, memo, msg);
        env::panic_str("role NFTs are non-transferable")
    }

    // ---------------------------------------------------------------------
    // NEP-245 enumeration
    // ---------------------------------------------------------------------

    /// NEP-245 enumeration: all configured role tokens.
    pub fn mt_tokens(&self, from_index: Option<U128>, limit: Option<u32>) -> Vec<MTRoleToken> {
        let (from, limit) = pagination(from_index, limit);
        self.roles
            .iter()
            .skip(from)
            .take(limit)
            .filter_map(|(role_id, _)| self.view_token(*role_id))
            .collect()
    }

    /// NEP-245 enumeration: role tokens held by an account.
    pub fn mt_tokens_for_owner(
        &self,
        account_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<MTRoleToken> {
        let (from, limit) = pagination(from_index, limit);
        self.owned_tokens
            .get(&account_id)
            .into_iter()
            .flatten()
            .skip(from)
            .take(limit)
            .filter_map(|role_id| self.view_token(*role_id))
            .collect()
    }

    // ---------------------------------------------------------------------
    // NEP-245 metadata
    // ---------------------------------------------------------------------

    /// NEP-245 metadata: contract-level metadata.
    pub fn mt_metadata_contract(&self) -> MTContractMetadata {
        MTContractMetadata {
            spec: "mt-1.0.0".to_owned(),
            name: "NEAR RBAC Role NFTs".to_owned(),
        }
    }

    /// NEP-245 metadata: base metadata by metadata id.
    pub fn mt_metadata_base_by_metadata_id(
        &self,
        base_metadata_ids: Vec<String>,
    ) -> Vec<MTBaseTokenMetadata> {
        base_metadata_ids
            .iter()
            .map(|id| {
                let role_id = parse_token_id(id)
                    .unwrap_or_else(|| env::panic_str("invalid base metadata id"));
                self.base_metadata_for_role(role_id)
            })
            .collect()
    }

    /// NEP-245 metadata: base metadata by token id.
    pub fn mt_metadata_base_by_token_id(&self, token_ids: Vec<String>) -> Vec<MTBaseTokenMetadata> {
        token_ids
            .iter()
            .map(|token_id| {
                let role_id =
                    parse_token_id(token_id).unwrap_or_else(|| env::panic_str("unknown token id"));
                self.base_metadata_for_role(role_id)
            })
            .collect()
    }

    /// NEP-245 metadata: full metadata by token id.
    pub fn mt_metadata_token_all(&self, token_ids: Vec<String>) -> Vec<MTTokenMetadataAll> {
        token_ids
            .iter()
            .map(|token_id| {
                let role_id =
                    parse_token_id(token_id).unwrap_or_else(|| env::panic_str("unknown token id"));
                MTTokenMetadataAll {
                    base: self.base_metadata_for_role(role_id),
                    token: self.token_metadata(role_id),
                }
            })
            .collect()
    }

    /// NEP-245 metadata: instance metadata by token id.
    pub fn mt_metadata_token_by_token_id(&self, token_ids: Vec<String>) -> Vec<MTTokenMetadata> {
        token_ids
            .iter()
            .map(|token_id| {
                let role_id =
                    parse_token_id(token_id).unwrap_or_else(|| env::panic_str("unknown token id"));
                self.token_metadata(role_id)
            })
            .collect()
    }

    /// NEP-245 metadata: all base metadata for configured roles.
    pub fn mt_tokens_base_metadata_all(
        &self,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<MTBaseTokenMetadata> {
        let (from, limit) = pagination(from_index, limit);
        self.roles
            .iter()
            .skip(from)
            .take(limit)
            .map(|(role_id, _)| self.base_metadata_for_role(*role_id))
            .collect()
    }

    // ---------------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------------

    fn assert_owner(&self) {
        require!(
            env::predecessor_account_id() == self.owner_id,
            "only the contract owner can call this method"
        );
    }

    fn assert_can_manage_role(&self, config: &Role) {
        let caller = env::predecessor_account_id();
        let effective = config.effective_owner(&self.owner_id);
        require!(
            caller == self.owner_id || caller == effective,
            "only the contract owner or role owner can manage this role"
        );
    }

    /// Computes the role id and enforces collision safety for a role name.
    fn get_or_create_role_id(&mut self, role_name: &str) -> u64 {
        validate_role_name(role_name)
            .unwrap_or_else(|err| env::panic_str(&format!("invalid role name: {err}")));
        let id = role_id(role_name);
        if let Some(existing) = self.roles.get(&id) {
            require!(
                existing.name == role_name,
                "role name hash collision; choose a different role name"
            );
        }
        if let Some(existing_id) = self.role_by_name.get(role_name) {
            require!(
                *existing_id == id,
                "role registry inconsistency; contact the contract owner"
            );
        }
        self.role_by_name.insert(role_name.to_owned(), id);
        id
    }

    fn role_view(&self, role_id: u64, config: &Role) -> RoleView {
        RoleView {
            role_id: U64(role_id),
            name: config.name.clone(),
            owner: config.effective_owner(&self.owner_id),
            configured_owner: config.owner.clone(),
            salary: U128(config.salary),
            holders: u64::from(self.holder_count(role_id)),
        }
    }

    fn holder_count(&self, role_id: u64) -> u32 {
        self.holder_count.get(&role_id).copied().unwrap_or(0)
    }

    fn add_holder(&mut self, role_id: u64, account_id: &AccountId) {
        let index = self.holder_count(role_id);
        self.role_holders
            .insert((role_id, index), account_id.clone());
        self.holder_index
            .insert((role_id, account_id.clone()), index);
        self.holder_count.insert(role_id, index + 1);
    }

    /// Removes a holder in O(1) by swapping the last holder into the freed slot.
    fn remove_holder(&mut self, role_id: u64, account_id: &AccountId) {
        let Some(index) = self
            .holder_index
            .get(&(role_id, account_id.clone()))
            .copied()
        else {
            return;
        };
        let count = self.holder_count(role_id);
        if count == 0 {
            return;
        }
        let last_index = count - 1;
        if index != last_index {
            if let Some(last_account) = self.role_holders.get(&(role_id, last_index)).cloned() {
                self.role_holders
                    .insert((role_id, index), last_account.clone());
                self.holder_index.insert((role_id, last_account), index);
            }
        }
        self.role_holders.remove(&(role_id, last_index));
        self.holder_index.remove(&(role_id, account_id.clone()));
        if last_index == 0 {
            self.holder_count.remove(&role_id);
        } else {
            self.holder_count.insert(role_id, last_index);
        }
    }

    /// Removes a membership from every index and emits the NEP-245 burn event.
    fn remove_member(
        &mut self,
        account_id: &AccountId,
        role_id: u64,
        authorized_id: Option<AccountId>,
    ) {
        if let Some(mut owned) = self.owned_tokens.get(account_id).cloned() {
            owned.retain(|id| *id != role_id);
            if owned.is_empty() {
                self.owned_tokens.remove(account_id);
            } else {
                self.owned_tokens.insert(account_id.clone(), owned);
            }
        }
        self.remove_holder(role_id, account_id);
        self.last_claim.remove(&(role_id, account_id.clone()));

        let token_id = token_id(role_id);
        events::emit_burn(account_id, &token_id, authorized_id);
    }

    fn is_member(&self, account_id: &AccountId, role_id: u64) -> bool {
        self.owned_tokens
            .get(account_id)
            .is_some_and(|roles| roles.contains(&role_id))
    }

    fn balance_of(&self, account_id: &AccountId, token_id: &str) -> u128 {
        parse_token_id(token_id)
            .map(|role_id| u128::from(self.is_member(account_id, role_id)))
            .unwrap_or(0)
    }

    fn view_token(&self, role_id: u64) -> Option<MTRoleToken> {
        self.roles.get(&role_id)?;
        Some(MTRoleToken {
            token_id: token_id(role_id),
            owner_id: None,
            base_metadata_id: base_metadata_id(role_id),
            token_metadata: Some(self.token_metadata(role_id)),
        })
    }

    fn base_metadata_for_role(&self, role_id: u64) -> MTBaseTokenMetadata {
        let role = self
            .roles
            .get(&role_id)
            .unwrap_or_else(|| env::panic_str("unknown role id"));
        MTBaseTokenMetadata {
            name: role.name.clone(),
            id: base_metadata_id(role_id),
            symbol: None,
            icon: None,
            decimals: Some("0".to_owned()),
            base_uri: None,
            reference: None,
            copies: Some(u64::from(self.holder_count(role_id))),
            reference_hash: None,
        }
    }

    fn token_metadata(&self, role_id: u64) -> MTTokenMetadata {
        let role = self
            .roles
            .get(&role_id)
            .unwrap_or_else(|| env::panic_str("unknown role id for token"));
        let holders = self.holder_count(role_id);
        let extra = near_sdk::serde_json::json!({
            "role": role.name,
            "role_id": role_id.to_string(),
            "salary": role.salary.to_string(),
            "payout_interval_ns": self.payout_interval_ns.to_string(),
            "holders": holders,
        });
        MTTokenMetadata {
            title: Some(format!("{} role", role.name)),
            description: Some(format!(
                "Non-transferable membership token for role '{}'. Each holder has a balance of 1.",
                role.name
            )),
            media: None,
            media_hash: None,
            issued_at: None,
            expires_at: None,
            starts_at: None,
            updated_at: None,
            extra: Some(extra.to_string()),
            reference: None,
            reference_hash: None,
        }
    }

    fn claimable_for_internal(&self, account_id: &AccountId, role_filter: Option<u64>) -> u128 {
        let now = env::block_timestamp();
        let interval = self.payout_interval_ns;
        let mut total = 0u128;
        for role_id in self.owned_tokens.get(account_id).into_iter().flatten() {
            if role_filter.is_some_and(|filter| filter != *role_id) {
                continue;
            }
            let Some(role) = self.roles.get(role_id) else {
                continue;
            };
            let last = self
                .last_claim
                .get(&(*role_id, account_id.clone()))
                .copied()
                .unwrap_or(now);
            let periods = now.saturating_sub(last) / interval;
            let amount = (periods as u128)
                .checked_mul(role.salary)
                .unwrap_or_else(|| env::panic_str("claim amount overflow"));
            total = total
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("total claim overflow"));
        }
        total
    }

    /// Accrued salary for one member in one role.
    fn claimable_role_amount(&self, account_id: &AccountId, role_id: u64) -> u128 {
        let Some(role) = self.roles.get(&role_id) else {
            return 0;
        };
        let now = env::block_timestamp();
        let last = self
            .last_claim
            .get(&(role_id, account_id.clone()))
            .copied()
            .unwrap_or(now);
        let periods = now.saturating_sub(last) / self.payout_interval_ns;
        (periods as u128)
            .checked_mul(role.salary)
            .unwrap_or_else(|| env::panic_str("claim amount overflow"))
    }

    /// Total salary owed to all current holders of a role at `now`.
    fn role_liability(&self, role_id: u64, now: u64) -> u128 {
        let Some(role) = self.roles.get(&role_id) else {
            return 0;
        };
        let interval = self.payout_interval_ns;
        let count = self.holder_count(role_id);
        let mut total = 0u128;
        for index in 0..count {
            let Some(holder) = self.role_holders.get(&(role_id, index)) else {
                continue;
            };
            let last = self
                .last_claim
                .get(&(role_id, holder.clone()))
                .copied()
                .unwrap_or(now);
            let periods = now.saturating_sub(last) / interval;
            let amount = (periods as u128)
                .checked_mul(role.salary)
                .unwrap_or_else(|| env::panic_str("liability overflow"));
            total = total
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("liability overflow"));
        }
        total
    }

    fn claim_internal(
        &mut self,
        account_id: &AccountId,
        role_filter: Option<u64>,
    ) -> PromiseOrValue<U128> {
        let now = env::block_timestamp();
        let interval = self.payout_interval_ns;
        let role_ids: Vec<u64> = self
            .owned_tokens
            .get(account_id)
            .cloned()
            .unwrap_or_default();
        let mut total = 0u128;
        let mut previous_last_claims = Vec::new();

        for role_id in role_ids {
            if role_filter.is_some_and(|filter| filter != role_id) {
                continue;
            }
            let Some(role) = self.roles.get(&role_id).cloned() else {
                continue;
            };
            let key = (role_id, account_id.clone());
            let last = self.last_claim.get(&key).copied().unwrap_or(now);
            let elapsed = now.saturating_sub(last);
            if elapsed < interval {
                continue;
            }
            let periods = elapsed / interval;
            let advance_ns = periods
                .checked_mul(interval)
                .unwrap_or_else(|| env::panic_str("claim timestamp overflow"));
            let next = last
                .checked_add(advance_ns)
                .unwrap_or_else(|| env::panic_str("claim timestamp overflow"));
            let amount = (periods as u128)
                .checked_mul(role.salary)
                .unwrap_or_else(|| env::panic_str("claim amount overflow"));
            total = total
                .checked_add(amount)
                .unwrap_or_else(|| env::panic_str("total claim overflow"));

            self.last_claim.insert(key, next);
            previous_last_claims.push((role_id, last));
        }

        if total == 0 {
            return PromiseOrValue::Value(U128(0));
        }

        let promise = ext_ft::ext(self.salary_token.clone())
            .with_attached_deposit(NearToken::from_yoctonear(1))
            .with_static_gas(Gas::from_tgas(30))
            .ft_transfer(account_id.clone(), U128(total), None)
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(Gas::from_tgas(25))
                    .on_claim_transfer(account_id.clone(), previous_last_claims, U128(total)),
            );
        PromiseOrValue::Promise(promise)
    }
}

fn pagination(from_index: Option<U128>, limit: Option<u32>) -> (usize, usize) {
    let from = from_index.map(|value| value.0).unwrap_or(0);
    let from = usize::try_from(from).unwrap_or(usize::MAX);
    let limit = limit
        .map(|value| value as usize)
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .min(MAX_PAGE_LIMIT);
    (from, limit)
}
