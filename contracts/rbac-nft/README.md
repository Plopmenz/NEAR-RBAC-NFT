# rbac-nft

Non-transferable **NEP-245** role NFTs with role-gated administration and **NEP-141 salary
streaming**. The contract is intended to be owned by a Sputnik DAO or Trezu account.

This crate is part of the [NEAR RBAC NFT](../../README.md) workspace. The root README has
the full project guide, OIDC flow, and security model.

## Identifiers

```text
role_id          = u64::from_be_bytes(SHA-256(role_name)[0..8])
token_id         = "role_id"              (decimal string)
base_metadata_id = "role_id"              (decimal string)
```

Each role is one NEP-245 token type. Membership is a balance of `1` on that token type, so
`mt_balance_of` returns `0`/`1`, `mt_supply` returns the holder count, and metadata is
role-level. Example: `admin` → `10117748766825055253`; that number is also the token id and
base metadata id. Role names are case-sensitive, at most 64 bytes, and must not have surrounding
whitespace or control characters. A hash collision is rejected when a role is configured.

## Initialization

```json
{
  "owner_id": "dao.testnet",
  "salary_token": "usdc.testnet",
  "payout_interval_ns": "604800000000000",
  "factory_id": null
}
```

`payout_interval_ns` is in nanoseconds: `86400000000000` is one day,
`604800000000000` is one week. `factory_id` is optional; the factory sets it when it deploys the
contract so `set_owner` can update the factory's owner index.

## Methods

### Configuration

- `owner_id()`, `salary_token()`, `payout_interval_ns()`, `factory_id()`
- `required_mint_deposit() -> U128` — storage deposit required by `mint_role`.
- `set_owner(new_owner_id) -> PromiseOrValue<bool>` — contract owner only. If the contract knows
  its factory, it also notifies the factory best-effort; the ownership change does not depend on
  that notification.
- `set_payout_config(salary_token, payout_interval_ns)` — contract owner only.
- `set_role_salary(role, salary) -> RoleView` — contract owner only; creates the role if
  needed.
- `set_role_owner(role, owner: Option<AccountId>) -> RoleView` — contract owner or the
  current effective role owner. `None` falls back to the contract owner.
- `get_role(role)`, `get_role_by_id(role_id: U64)`, `get_roles(from_index?, limit?)`,
  `role_count()`.
- `RoleView.role_id` is a `U64` decimal string.

### Membership

- `mint_role(account_id, role) -> String` — payable; contract owner or role owner. The role must
  be configured. Attach at least `required_mint_deposit()`; the deposit is a non-refundable
  storage reserve. Returns the decimal role id / token id. Starts salary accrual at the current
  block timestamp.
- `burn_role(account_id, role, pay_pending: Option<bool>) -> PromiseOrValue<bool>` — contract
  owner or role owner. Access stops immediately. By default (`None` or `true`) accrued salary is
  paid to the member first; pass `false` to forfeit it. If the FT transfer fails, the membership
  and claim timestamp are restored.
- `has_role(account_id, role) -> bool`
- `has_role_id(account_id, role_id: U64) -> bool`
- `roles_of(account_id) -> Vec<String>`
- `role_ids_of(account_id) -> Vec<U64>`
- `members_of(role, from_index?, limit?) -> Vec<AccountId>` — paginated; default limit 50,
  maximum 100.

An account can hold each role once and at most 32 roles in total. The limit is configurable at
build time via the compile-time constant `MAX_ROLES_PER_ACCOUNT` in `src/lib.rs`; it is not a
runtime or owner setting. 32 is deliberately conservative: `claim` loops over every role held, so
the cap bounds gas use and response size, and `claim_role` can claim a single role. Raising the
limit requires rebuilding and redeploying the contract.

### Salary

- `claim(account_id) -> PromiseOrValue<U128>` — anyone may call; the payout goes to
  `account_id`.
- `claim_role(account_id, role) -> PromiseOrValue<U128>`
- `claimable(account_id) -> U128`
- `claimable_role(account_id, role) -> U128`
- `last_claim(account_id, role) -> Option<U64>`
- `accrued_liabilities() -> U128` — total salary accrued but not yet claimed.
- `accrued_liabilities_for_role(role) -> U128` — liability for one role.
- `withdraw_ft(token, receiver_id, amount) -> Promise` — contract owner only; recovers any
  NEP-141 token, including accidental transfers. Check `accrued_liabilities()` first when the
  token is the salary token.
- `on_claim_transfer(...) -> bool` and `on_burn_transfer(...) -> bool` — private callbacks.

Only full intervals are paid. A claim advances the timestamp by `periods * interval`, so
partial time carries over. Minting starts accrual at mint time. Holders of the same role accrue
independently. Salary and interval changes are retroactive to unclaimed elapsed time, so claim
before reconfiguring payroll if that matters.

The payout calls the salary token's `ft_transfer` with 1 yoctoNEAR attached. On failure, the
private callback restores the previous timestamps so the member can retry. `burn_role` uses the
same mechanism: a failed payout leaves the membership in place with its accrued salary intact.

### NEP-245

Implemented reads and enumeration:

- `mt_token`, `mt_balance_of`, `mt_batch_balance_of`, `mt_supply`, `mt_batch_supply`
- `mt_tokens`, `mt_tokens_for_owner`
- `mt_metadata_contract`, `mt_metadata_base_by_metadata_id`,
  `mt_metadata_base_by_token_id`, `mt_metadata_token_all`,
  `mt_metadata_token_by_token_id`, `mt_tokens_base_metadata_all`

The transfer entry points (`mt_transfer`, `mt_batch_transfer`, `mt_transfer_call`,
`mt_batch_transfer_call`) always panic with `role NFTs are non-transferable`. Approvals are
not implemented because memberships are not transferable. `mt_supply` returns the holder count
for a configured role (`Some(0)` when empty) and `None` for an unknown role; `mt_tokens`
enumerates configured roles, not memberships.

## Events

Mints and burns emit NEP-297 `nep245` events:

```json
EVENT_JSON:{"standard":"nep245","version":"1.0.0","event":"mt_mint","data":[{"owner_id":"alice.testnet","token_ids":["10117748766825055253"],"amounts":["1"]}]}
```

Burns use event `mt_burn` and include `authorized_id` when the caller is not the member.

## Example calls

```bash
near call rbac.testnet set_role_salary \
  '{"role":"admin","salary":"1000000000000000000000000"}' \
  --accountId dao.testnet

near call rbac.testnet set_role_owner \
  '{"role":"admin","owner":"hr.testnet"}' \
  --accountId dao.testnet

near call rbac.testnet mint_role \
  '{"account_id":"alice.testnet","role":"admin"}' \
  --accountId hr.testnet \
  --deposit 0.011

# Burn removes access and pays accrued salary first. Pass `"pay_pending": false` to forfeit it.
near call rbac.testnet burn_role \
  '{"account_id":"alice.testnet","role":"admin","pay_pending":true}' \
  --accountId hr.testnet \
  --gas 150000000000000

near view rbac.testnet has_role '{"account_id":"alice.testnet","role":"admin"}'
near view rbac.testnet roles_of '{"account_id":"alice.testnet"}'

near call rbac.testnet claim \
  '{"account_id":"alice.testnet"}' \
  --accountId alice.testnet \
  --gas 100000000000000
```

## Limitations

- Not audited.
- 64-bit role ids; collisions are detected at configuration time.
- Maximum 32 roles per account. `MAX_ROLES_PER_ACCOUNT` is a compile-time constant; raising it
  requires a rebuild/redeploy and increases the per-account `claim` gas and response size.
- Roles cannot be renamed or deleted.
- Burning pays accrued salary by default. If that transfer fails, the burn is rolled back; pass
  `pay_pending=false` to forfeit the salary and remove the membership without an FT call.
- Minting requires a non-refundable storage deposit reported by `required_mint_deposit()`.
- Payouts require the contract to hold the salary token and the member to be registered with
  that token contract. There is no retry queue.
- `withdraw_ft` is owner-only and can move salary funds; compare the amount with
  `accrued_liabilities()` before withdrawing the salary token.
- The callback reads at most 1 KiB of the promise result.
- `claim` loops over the member's roles in one call.

See the root [README](../../README.md#limitations) for the project-wide limitations.
