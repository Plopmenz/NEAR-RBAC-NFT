#![doc = include_str!("../README.md")]

mod events;

#[cfg(test)]
mod tests;

use near_sdk::json_types::{Base64VecU8, U128, U64};
use near_sdk::store::{IterableMap, LookupMap};
use near_sdk::{env, near, require, AccountId, Gas, NearToken, PanicOnDefault, Promise};

/// Bytes reserved in the factory for one registry entry (record + owner/deployer indexes).
const REGISTRY_RESERVE_BYTES: u128 = 2_048;
/// Bytes reserved in the new child account for the empty contract state.
const CHILD_STATE_RESERVE_BYTES: u128 = 1_000;
/// Gas reserved for the child `new` call and the factory callback.
const DEPLOY_CALL_GAS: Gas = Gas::from_tgas(60);
const DEPLOY_CALLBACK_GAS: Gas = Gas::from_tgas(30);
/// Minimum prepaid gas needed to create the account, deploy the wasm, and initialize it.
const MIN_DEPLOY_PREPAID_GAS: Gas = Gas::from_tgas(150);

const DEFAULT_PAGE_LIMIT: usize = 50;
const MAX_PAGE_LIMIT: usize = 100;

/// A registry entry for a deployed RBAC contract.
///
/// `owner_id` tracks the child's current owner: it is set at deployment and updated when a
/// factory-deployed child calls [`Contract::on_owner_changed`] after `set_owner`. The update is
/// best-effort, so clients that need the authoritative owner should query the child's
/// `owner_id()` view.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractRecord {
    pub account_id: AccountId,
    /// Subaccount label, without the factory suffix.
    pub name: String,
    /// Current owner, as reported by the child. See the note above about best-effort updates.
    pub owner_id: AccountId,
    pub salary_token: AccountId,
    pub payout_interval_ns: U64,
    /// Account that paid for and triggered the deployment.
    pub deployer_id: AccountId,
    pub deployed_at_ns: U64,
}

/// Factory that deploys RBAC role NFT contracts as subaccounts and keeps a registry of them.
///
/// The factory is deliberately immutable and keyless after initialization: it has no owner, no
/// admin methods, and no way to replace the stored child wasm. The code is fixed at creation time
/// by [`Contract::new`], so every deployment is verifiable against the factory's `code_hash`.
#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Contract {
    /// rbac-nft wasm stored once at initialization. Never modified.
    code: Vec<u8>,
    /// child account -> record. Iterable for discovery.
    records: IterableMap<AccountId, ContractRecord>,
    /// owner -> child accounts. Updated on owner changes when the child notifies the factory.
    by_owner: LookupMap<AccountId, Vec<AccountId>>,
}

#[near]
impl Contract {
    /// Creates the factory and stores the child wasm permanently.
    ///
    /// `code` is a base64-encoded rbac-nft wasm file. The attached deposit must cover the storage
    /// for the code; the factory keeps that stake for as long as the code is stored. There is no
    /// owner and no way to replace the code after this call.
    #[init]
    #[payable]
    pub fn new(code: Base64VecU8) -> Self {
        let code: Vec<u8> = code.into();
        require!(!code.is_empty(), "code must not be empty");
        let storage = env::storage_byte_cost().saturating_mul(code.len() as u128);
        require!(
            env::attached_deposit() >= storage,
            "attached deposit must cover code storage"
        );
        Self {
            code,
            records: IterableMap::new(b"records".as_slice()),
            by_owner: LookupMap::new(b"by_owner".as_slice()),
        }
    }

    // ---------------------------------------------------------------------
    // Stored child code
    // ---------------------------------------------------------------------

    /// Returns the length of the stored child wasm in bytes.
    pub fn code_len(&self) -> U64 {
        U64(self.code.len() as u64)
    }

    /// Returns the SHA-256 hash of the stored child wasm as lowercase hex.
    pub fn code_hash(&self) -> String {
        let hash = env::sha256(&self.code);
        hash.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Returns the deposit required to deploy one child contract at the current storage price.
    ///
    /// The factory keeps a fixed reserve for the registry entry and forwards the rest to the child
    /// account, which must stake the wasm and its initial state.
    pub fn required_deploy_deposit(&self) -> U128 {
        let bytes = self.code.len() as u128 + CHILD_STATE_RESERVE_BYTES + REGISTRY_RESERVE_BYTES;
        U128(
            env::storage_byte_cost()
                .as_yoctonear()
                .saturating_mul(bytes),
        )
    }

    // ---------------------------------------------------------------------
    // Deployment
    // ---------------------------------------------------------------------

    /// Deploys a new rbac-nft contract at `{name}.{factory}` and initializes it.
    ///
    /// `owner_id` becomes the child contract's owner. The attached deposit must be at least
    /// [`Contract::required_deploy_deposit`]; excess is forwarded to the child. The caller must
    /// attach enough gas for the wasm deployment and initialization (at least 150 Tgas).
    ///
    /// The factory registers the child only after its `new` call succeeds. A failed deployment
    /// leaves an uninitialized account at the target id, because the factory has no key for the
    /// new account and cannot delete it. Check the transaction outcome and use a different name
    /// if that happens.
    #[payable]
    pub fn deploy(
        &mut self,
        name: String,
        owner_id: AccountId,
        salary_token: AccountId,
        payout_interval_ns: U64,
    ) -> Promise {
        require!(!self.code.is_empty(), "factory code is not configured");
        require!(payout_interval_ns.0 > 0, "payout interval must be positive");
        require!(
            env::prepaid_gas() >= MIN_DEPLOY_PREPAID_GAS,
            "at least 150 Tgas is required to deploy"
        );

        let factory = env::current_account_id();
        validate_subaccount_name(&name, factory.as_str());
        let account_id: AccountId = format!("{name}.{factory}")
            .parse()
            .unwrap_or_else(|_| env::panic_str("invalid child account id"));
        require!(
            !self.records.contains_key(&account_id),
            "contract is already registered"
        );

        let required = self.required_deploy_deposit().0;
        require!(
            env::attached_deposit() >= NearToken::from_yoctonear(required),
            "attached deposit is too low; call required_deploy_deposit"
        );

        // Keep a fixed reserve in the factory for the registry entry and forward the rest to the
        // child. Any unused reserve stays with the factory, which has no withdrawal method.
        let reserve = env::storage_byte_cost().saturating_mul(REGISTRY_RESERVE_BYTES);
        let child_balance = env::attached_deposit().saturating_sub(reserve);
        let deployer_id = env::predecessor_account_id();
        let deployed_at_ns = U64(env::block_timestamp());

        let init_args = near_sdk::serde_json::json!({
            "owner_id": owner_id,
            "salary_token": salary_token,
            "payout_interval_ns": payout_interval_ns.0.to_string(),
            "factory_id": factory,
        })
        .to_string()
        .into_bytes();

        let record = ContractRecord {
            account_id: account_id.clone(),
            name,
            owner_id,
            salary_token,
            payout_interval_ns,
            deployer_id,
            deployed_at_ns,
        };

        Promise::new(account_id)
            .create_account()
            .deploy_contract(self.code.clone())
            .transfer(child_balance)
            .function_call(
                "new".to_owned(),
                init_args,
                NearToken::from_yoctonear(0),
                DEPLOY_CALL_GAS,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(DEPLOY_CALLBACK_GAS)
                    .on_deploy(record),
            )
    }

    /// Private callback for [`Contract::deploy`]. Records the child only when initialization
    /// succeeded.
    ///
    /// If initialization fails, the factory refunds its registry reserve to the deployer. The
    /// forwarded child balance cannot be recovered: the child account exists without keys, and
    /// the subaccount name cannot be reused.
    #[private]
    pub fn on_deploy(&mut self, record: ContractRecord) -> bool {
        require!(
            env::promise_results_count() == 1,
            "expected exactly one promise result"
        );
        if env::promise_result_checked(0, 1_024).is_err() {
            self.refund_reserve(&record.deployer_id);
            events::emit_deploy_failed(record.account_id.as_str(), "initialization failed");
            return false;
        }
        if self.records.contains_key(&record.account_id) {
            self.refund_reserve(&record.deployer_id);
            events::emit_deploy_failed(record.account_id.as_str(), "already registered");
            return false;
        }

        let account_id = record.account_id.clone();
        let owner_id = record.owner_id.clone();

        self.records.insert(account_id.clone(), record.clone());
        push_index(&mut self.by_owner, owner_id, account_id);

        events::emit_deploy(&record);
        true
    }

    /// Updates the registry when a child contract transfers ownership.
    ///
    /// Only a registered child can call this, because the caller is the child account itself.
    /// Factory-deployed children have no access keys, so only their own `set_owner` method can
    /// produce this call. The child treats the notification as best-effort, so this index can be
    /// stale if the call fails; clients should verify the child's `owner_id()`.
    pub fn on_owner_changed(&mut self, new_owner_id: AccountId) -> bool {
        let child = env::predecessor_account_id();
        let Some(mut record) = self.records.get(&child).cloned() else {
            env::panic_str("caller is not a registered child");
        };
        if record.owner_id != new_owner_id {
            let old_owner = record.owner_id.clone();
            remove_index(&mut self.by_owner, &old_owner, &child);
            push_index(&mut self.by_owner, new_owner_id.clone(), child.clone());
            record.owner_id = new_owner_id;
            self.records.insert(child, record);
        }
        true
    }

    /// Refunds the factory's registry reserve to the deployer after a failed registration.
    fn refund_reserve(&self, deployer_id: &AccountId) {
        let reserve = env::storage_byte_cost().saturating_mul(REGISTRY_RESERVE_BYTES);
        if reserve.as_yoctonear() > 0 {
            let _ = Promise::new(deployer_id.clone()).transfer(reserve);
        }
    }

    // ---------------------------------------------------------------------
    // Discovery
    // ---------------------------------------------------------------------

    /// Total number of successfully deployed and registered contracts.
    pub fn contract_count(&self) -> u64 {
        u64::from(self.records.len())
    }

    /// Returns one registry entry by child account id.
    pub fn get_contract(&self, account_id: AccountId) -> Option<ContractRecord> {
        self.records.get(&account_id).cloned()
    }

    /// Lists all registered contracts with pagination.
    pub fn get_contracts(
        &self,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<ContractRecord> {
        let (from, limit) = pagination(from_index, limit);
        self.records
            .iter()
            .skip(from)
            .take(limit)
            .map(|(_, record)| record.clone())
            .collect()
    }

    /// Lists contracts whose current owner is `owner_id`.
    ///
    /// This index is advisory. A child updates it best-effort after `set_owner`; if that call
    /// fails, the index can be stale. Anyone can also deploy a child with an arbitrary initial
    /// `owner_id`, which adds that child to the named owner's list without their consent. Clients
    /// must verify the live `owner_id()` on the child before acting on a result.
    pub fn contracts_of_owner(
        &self,
        owner_id: AccountId,
        from_index: Option<U128>,
        limit: Option<u32>,
    ) -> Vec<ContractRecord> {
        let (from, limit) = pagination(from_index, limit);
        self.by_owner
            .get(&owner_id)
            .into_iter()
            .flatten()
            .skip(from)
            .take(limit)
            .filter_map(|account_id| self.records.get(account_id).cloned())
            .collect()
    }
}

fn push_index(map: &mut LookupMap<AccountId, Vec<AccountId>>, key: AccountId, value: AccountId) {
    let mut values = map.get(&key).cloned().unwrap_or_default();
    if !values.contains(&value) {
        values.push(value);
        map.insert(key, values);
    }
}

fn remove_index(
    map: &mut LookupMap<AccountId, Vec<AccountId>>,
    key: &AccountId,
    value: &AccountId,
) {
    let Some(mut values) = map.get(key).cloned() else {
        return;
    };
    values.retain(|entry| entry != value);
    if values.is_empty() {
        map.remove(key);
    } else {
        map.insert(key.clone(), values);
    }
}

fn validate_subaccount_name(name: &str, factory: &str) {
    require!(!name.is_empty(), "name must not be empty");
    require!(
        name.len() + 1 + factory.len() <= 64,
        "name is too long for a subaccount"
    );
    require!(
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'),
        "name may only contain lowercase letters, digits, '-' and '_'"
    );
    require!(
        !name.starts_with('-') && !name.starts_with('_'),
        "name must start with a letter or digit"
    );
    require!(
        !name.ends_with('-') && !name.ends_with('_'),
        "name must end with a letter or digit"
    );
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
