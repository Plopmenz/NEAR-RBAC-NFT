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
use rbac_common::{
    base_metadata_id, parse_token_id, role_id, token_id_from_role_id, validate_role_name,
};

use crate::metadata::{
    MTBaseTokenMetadata, MTContractMetadata, MTRoleToken, MTTokenMetadata, MTTokenMetadataAll,
};

/// Maximum number of role memberships one account may hold.
///
/// `claim` iterates a member's roles in one call. Capping the number keeps claims well inside the
/// block gas limit and makes salary payout predictable.
pub const MAX_ROLES_PER_ACCOUNT: usize = 32;

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

/// Stored data for one role membership token.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct TokenData {
    pub role_id: u32,
    pub owner: AccountId,
    pub minted_at_ns: u64,
}

/// Public view of a role.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleView {
    pub role_id: u32,
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

    /// role id -> role configuration. Iterable for metadata/enumeration.
    roles: IterableMap<u32, Role>,
    /// role name -> role id, used for collision detection and lookups.
    role_by_name: LookupMap<String, u32>,
    /// token id -> token data. Iterable for NEP-245 enumeration.
    tokens: IterableMap<String, TokenData>,
    /// account -> token ids. Kept flat because the number of roles per account is capped.
    owned_tokens: LookupMap<AccountId, Vec<String>>,
    /// role id -> member accounts. Used for role administration views.
    role_holders: LookupMap<u32, Vec<AccountId>>,
    /// token id -> last salary claim timestamp in nanoseconds.
    last_claim: LookupMap<String, u64>,
}

#[near]
impl Contract {
    /// Creates the extension.
    ///
    /// `owner_id` should normally be the Sputnik DAO or Trezu account that governs this
    /// extension. `salary_token` is the NEP-141 token the contract pays salaries in, and
    /// `payout_interval_ns` is the global payout period (`1_000_000_000` = one second,
    /// `604_800_000_000_000` = one week).
    #[init]
    pub fn new(owner_id: AccountId, salary_token: AccountId, payout_interval_ns: U64) -> Self {
        require!(payout_interval_ns.0 > 0, "payout interval must be positive");
        Self {
            owner_id,
            salary_token,
            payout_interval_ns: payout_interval_ns.0,
            roles: IterableMap::new(b"roles".as_slice()),
            role_by_name: LookupMap::new(b"role_names".as_slice()),
            tokens: IterableMap::new(b"tokens".as_slice()),
            owned_tokens: LookupMap::new(b"owned_tokens".as_slice()),
            role_holders: LookupMap::new(b"role_holders".as_slice()),
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

    /// Transfers ownership of the extension. Only the current owner may call this.
    pub fn set_owner(&mut self, new_owner_id: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner_id;
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
    /// Role names are hashed to a 32-bit id. If the id is already used by a different role name,
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

    /// Returns a role by its 32-bit role id, if configured.
    pub fn get_role_by_id(&self, role_id: u32) -> Option<RoleView> {
        self.roles
            .get(&role_id)
            .map(|config| self.role_view(role_id, config))
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
    /// once. The token id is `{role_id}:{account_id}` and the base metadata id is the role id.
    /// Minting starts salary accrual at the current block timestamp.
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

        let token_id = token_id_from_role_id(role_id, account_id.as_str());
        require!(
            !self.tokens.contains_key(&token_id),
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
        owned.push(token_id.clone());
        self.owned_tokens.insert(account_id.clone(), owned);

        let mut holders = self.role_holders.get(&role_id).cloned().unwrap_or_default();
        holders.push(account_id.clone());
        self.role_holders.insert(role_id, holders);

        let now = env::block_timestamp();
        self.tokens.insert(
            token_id.clone(),
            TokenData {
                role_id,
                owner: account_id.clone(),
                minted_at_ns: now,
            },
        );
        self.last_claim.insert(token_id.clone(), now);

        events::emit_mint(&account_id, &token_id);
        token_id
    }

    /// Burns an account's role NFT.
    ///
    /// Callable by the contract owner or the role owner. Access stops immediately and unclaimed
    /// salary for this role is discarded; call `claim` first if the member has accrued pay.
    pub fn burn_role(&mut self, account_id: AccountId, role: String) {
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

        let token_id = token_id_from_role_id(role_id, account_id.as_str());
        let token = self
            .tokens
            .get(&token_id)
            .cloned()
            .unwrap_or_else(|| env::panic_str("account does not have this role"));
        require!(token.owner == account_id, "token owner mismatch");

        self.tokens.remove(&token_id);
        self.last_claim.remove(&token_id);
        if let Some(mut owned) = self.owned_tokens.get(&account_id).cloned() {
            owned.retain(|id| id != &token_id);
            if owned.is_empty() {
                self.owned_tokens.remove(&account_id);
            } else {
                self.owned_tokens.insert(account_id.clone(), owned);
            }
        }
        if let Some(mut holders) = self.role_holders.get(&role_id).cloned() {
            holders.retain(|account| account != &account_id);
            if holders.is_empty() {
                self.role_holders.remove(&role_id);
            } else {
                self.role_holders.insert(role_id, holders);
            }
        }

        let caller = env::predecessor_account_id();
        let authorized_id = (caller != account_id).then_some(caller);
        events::emit_burn(&account_id, &token_id, authorized_id);
    }

    /// Returns true when `account_id` currently holds `role`.
    ///
    /// This is the off-chain/check helper used by services. On-chain contracts should use the
    /// same token-id check at the start of their own methods (see README) or call this contract
    /// asynchronously before performing a gated action.
    pub fn has_role(&self, account_id: AccountId, role: String) -> bool {
        self.has_role_id(account_id, role_id(&role))
    }

    /// Returns true when `account_id` holds the role identified by `role_id`.
    ///
    /// `role_id` is `u32::from_be_bytes(SHA-256(role_name)[0..4])`; it is also the NEP-245 base
    /// metadata id of the role.
    pub fn has_role_id(&self, account_id: AccountId, role_id: u32) -> bool {
        let token_id = token_id_from_role_id(role_id, account_id.as_str());
        self.tokens
            .get(&token_id)
            .map(|token| token.owner == account_id)
            .unwrap_or(false)
    }

    /// Returns the human-readable roles held by an account. Used by the OIDC service.
    pub fn roles_of(&self, account_id: AccountId) -> Vec<String> {
        let mut roles: Vec<String> = self
            .owned_tokens
            .get(&account_id)
            .into_iter()
            .flatten()
            .filter_map(|token_id| parse_token_id(token_id).map(|(role_id, _)| role_id))
            .filter_map(|role_id| self.roles.get(&role_id).map(|role| role.name.clone()))
            .collect();
        roles.sort();
        roles.dedup();
        roles
    }

    /// Returns the role ids held by an account.
    pub fn role_ids_of(&self, account_id: AccountId) -> Vec<u32> {
        let mut role_ids: Vec<u32> = self
            .owned_tokens
            .get(&account_id)
            .into_iter()
            .flatten()
            .filter_map(|token_id| parse_token_id(token_id).map(|(role_id, _)| role_id))
            .collect();
        role_ids.sort_unstable();
        role_ids
    }

    /// Returns the members of a role.
    pub fn members_of(&self, role: String) -> Vec<AccountId> {
        let role_id = role_id(&role);
        let mut members = self.role_holders.get(&role_id).cloned().unwrap_or_default();
        members.sort();
        members
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
        let token_id = token_id_from_role_id(role_id(&role), account_id.as_str());
        self.last_claim.get(&token_id).copied().map(U64)
    }

    /// Called by the salary token transfer promise. Restores claim timestamps if the transfer
    /// failed. Restricted to self.
    #[private]
    pub fn on_claim_transfer(
        &mut self,
        account_id: AccountId,
        previous_last_claims: Vec<(u32, u64)>,
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
                let token_id = token_id_from_role_id(role_id, account_id.as_str());
                if self.tokens.contains_key(&token_id) {
                    self.last_claim.insert(token_id, previous);
                }
            }
            false
        }
    }

    // ---------------------------------------------------------------------
    // NEP-245 core (read + non-transferable mutators)
    // ---------------------------------------------------------------------

    /// NEP-245: returns tokens for the given token ids.
    pub fn mt_token(&self, token_ids: Vec<String>) -> Vec<Option<MTRoleToken>> {
        token_ids
            .iter()
            .map(|token_id| self.view_token(token_id))
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

    /// NEP-245: supply of a token id. Role NFTs have supply 0 or 1.
    pub fn mt_supply(&self, token_id: String) -> Option<U128> {
        self.tokens.contains_key(&token_id).then_some(U128(1))
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

    /// NEP-245 enumeration: all tokens in the contract.
    pub fn mt_tokens(&self, from_index: Option<U128>, limit: Option<u32>) -> Vec<MTRoleToken> {
        let (from, limit) = pagination(from_index, limit);
        self.tokens
            .iter()
            .skip(from)
            .take(limit)
            .filter_map(|(token_id, token)| self.view_token_with_data(token_id, token))
            .collect()
    }

    /// NEP-245 enumeration: tokens owned by an account.
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
            .filter_map(|token_id| self.view_token(token_id))
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
                let role_id = id
                    .parse::<u32>()
                    .unwrap_or_else(|_| env::panic_str("invalid base metadata id"));
                self.base_metadata_for_role(role_id)
            })
            .collect()
    }

    /// NEP-245 metadata: base metadata by token id.
    pub fn mt_metadata_base_by_token_id(&self, token_ids: Vec<String>) -> Vec<MTBaseTokenMetadata> {
        token_ids
            .iter()
            .map(|token_id| {
                let token = self
                    .tokens
                    .get(token_id)
                    .unwrap_or_else(|| env::panic_str("unknown token id"));
                self.base_metadata_for_role(token.role_id)
            })
            .collect()
    }

    /// NEP-245 metadata: full metadata by token id.
    pub fn mt_metadata_token_all(&self, token_ids: Vec<String>) -> Vec<MTTokenMetadataAll> {
        token_ids
            .iter()
            .map(|token_id| {
                let token = self
                    .tokens
                    .get(token_id)
                    .unwrap_or_else(|| env::panic_str("unknown token id"));
                MTTokenMetadataAll {
                    base: self.base_metadata_for_role(token.role_id),
                    token: self.token_metadata(token),
                }
            })
            .collect()
    }

    /// NEP-245 metadata: instance metadata by token id.
    pub fn mt_metadata_token_by_token_id(&self, token_ids: Vec<String>) -> Vec<MTTokenMetadata> {
        token_ids
            .iter()
            .map(|token_id| {
                let token = self
                    .tokens
                    .get(token_id)
                    .unwrap_or_else(|| env::panic_str("unknown token id"));
                self.token_metadata(token)
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
    fn get_or_create_role_id(&mut self, role_name: &str) -> u32 {
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

    fn role_view(&self, role_id: u32, config: &Role) -> RoleView {
        RoleView {
            role_id,
            name: config.name.clone(),
            owner: config.effective_owner(&self.owner_id),
            configured_owner: config.owner.clone(),
            salary: U128(config.salary),
            holders: self
                .role_holders
                .get(&role_id)
                .map(|holders| holders.len() as u64)
                .unwrap_or(0),
        }
    }

    fn balance_of(&self, account_id: &AccountId, token_id: &str) -> u128 {
        self.tokens
            .get(token_id)
            .map(|token| u128::from(token.owner == *account_id))
            .unwrap_or(0)
    }

    fn view_token(&self, token_id: &str) -> Option<MTRoleToken> {
        let token = self.tokens.get(token_id)?;
        self.view_token_with_data(token_id, token)
    }

    fn view_token_with_data(&self, token_id: &str, token: &TokenData) -> Option<MTRoleToken> {
        Some(MTRoleToken {
            token_id: token_id.to_owned(),
            owner_id: Some(token.owner.clone()),
            base_metadata_id: base_metadata_id(token.role_id),
            token_metadata: None,
        })
    }

    fn base_metadata_for_role(&self, role_id: u32) -> MTBaseTokenMetadata {
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
            copies: Some(
                self.role_holders
                    .get(&role_id)
                    .map(|holders| holders.len() as u64)
                    .unwrap_or(0),
            ),
            reference_hash: None,
        }
    }

    fn token_metadata(&self, token: &TokenData) -> MTTokenMetadata {
        let role = self
            .roles
            .get(&token.role_id)
            .unwrap_or_else(|| env::panic_str("unknown role id for token"));
        let extra = near_sdk::serde_json::json!({
            "role": role.name,
            "role_id": token.role_id,
            "salary": role.salary.to_string(),
            "payout_interval_ns": self.payout_interval_ns.to_string(),
        });
        MTTokenMetadata {
            title: Some(format!("{} role", role.name)),
            description: Some(format!(
                "Non-transferable membership token for role '{}' held by {}.",
                role.name, token.owner
            )),
            media: None,
            media_hash: None,
            issued_at: Some((token.minted_at_ns / 1_000_000).to_string()),
            expires_at: None,
            starts_at: None,
            updated_at: None,
            extra: Some(extra.to_string()),
            reference: None,
            reference_hash: None,
        }
    }

    fn claimable_for_internal(&self, account_id: &AccountId, role_filter: Option<u32>) -> u128 {
        let now = env::block_timestamp();
        let interval = self.payout_interval_ns;
        let mut total = 0u128;
        for token_id in self.owned_tokens.get(account_id).into_iter().flatten() {
            let Some((role_id, owner)) = parse_token_id(token_id) else {
                continue;
            };
            if owner != account_id.as_str() || role_filter.is_some_and(|filter| filter != role_id) {
                continue;
            }
            let Some(role) = self.roles.get(&role_id) else {
                continue;
            };
            let last = self.last_claim.get(token_id).copied().unwrap_or(now);
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

    fn claim_internal(
        &mut self,
        account_id: &AccountId,
        role_filter: Option<u32>,
    ) -> PromiseOrValue<U128> {
        let now = env::block_timestamp();
        let interval = self.payout_interval_ns;
        let token_ids: Vec<String> = self
            .owned_tokens
            .get(account_id)
            .cloned()
            .unwrap_or_default();
        let mut total = 0u128;
        let mut previous_last_claims = Vec::new();

        for token_id in token_ids {
            let Some((role_id, owner)) = parse_token_id(&token_id) else {
                continue;
            };
            if owner != account_id.as_str() || role_filter.is_some_and(|filter| filter != role_id) {
                continue;
            }
            let Some(role) = self.roles.get(&role_id).cloned() else {
                continue;
            };
            let last = self.last_claim.get(&token_id).copied().unwrap_or(now);
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

            self.last_claim.insert(token_id, next);
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
