# rbac-nft

Non-transferable **NEP-245** role NFTs with role-gated administration and **NEP-141 salary
streaming**. The contract is intended to be owned by a Sputnik DAO or Trezu account.

This crate is part of the [NEAR RBAC NFT](../../README.md) workspace. The root README has
the full project guide, OIDC flow, and security model.

## Identifiers

```text
role_id          = u32::from_be_bytes(SHA-256(role_name)[0..4])
token_id         = "{role_id}:{account_id}"
base_metadata_id = "{role_id}"
```

Example: `admin` → `2355721957`, so `alice.testnet`'s admin token is
`2355721957:alice.testnet`. Role names are case-sensitive, at most 64 bytes, and must not
have surrounding whitespace or control characters. A 32-bit collision is rejected when a
role is configured.

## Initialization

```json
{
  "owner_id": "dao.testnet",
  "salary_token": "usdc.testnet",
  "payout_interval_ns": "604800000000000"
}
```

`payout_interval_ns` is in nanoseconds: `86400000000000` is one day,
`604800000000000` is one week.

## Methods

### Configuration

- `owner_id()`, `salary_token()`, `payout_interval_ns()`
- `set_owner(new_owner_id)` — contract owner only.
- `set_payout_config(salary_token, payout_interval_ns)` — contract owner only.
- `set_role_salary(role, salary) -> RoleView` — contract owner only; creates the role if
  needed.
- `set_role_owner(role, owner: Option<AccountId>) -> RoleView` — contract owner or the
  current effective role owner. `None` falls back to the contract owner.
- `get_role(role)`, `get_role_by_id(role_id)`, `get_roles(from_index?, limit?)`,
  `role_count()`.

### Membership

- `mint_role(account_id, role) -> String` — contract owner or role owner. The role must be
  configured. Starts salary accrual at the current block timestamp.
- `burn_role(account_id, role)` — contract owner or role owner. Access stops immediately and
  unclaimed salary is discarded.
- `has_role(account_id, role) -> bool`
- `has_role_id(account_id, role_id) -> bool`
- `roles_of(account_id) -> Vec<String>`
- `role_ids_of(account_id) -> Vec<u32>`
- `members_of(role) -> Vec<AccountId>`

An account can hold each role once and at most 32 roles in total. The cap keeps `claim`
inside the block gas limit.

### Salary

- `claim(account_id) -> PromiseOrValue<U128>` — anyone may call; the payout goes to
  `account_id`.
- `claim_role(account_id, role) -> PromiseOrValue<U128>`
- `claimable(account_id) -> U128`
- `claimable_role(account_id, role) -> U128`
- `last_claim(account_id, role) -> Option<U64>`
- `on_claim_transfer(account_id, previous_last_claims, amount) -> bool` — private callback.

Only full intervals are paid. A claim advances the timestamp by `periods * interval`, so
partial time carries over. Minting starts accrual at mint time; burning forfeits unclaimed
salary. Salary and interval changes are retroactive to unclaimed elapsed time, so claim
before reconfiguring payroll if that matters.

The payout calls the salary token's `ft_transfer` with 1 yoctoNEAR attached. On failure, the
private callback restores the previous timestamps so the member can retry.

### NEP-245

Implemented reads and enumeration:

- `mt_token`, `mt_balance_of`, `mt_batch_balance_of`, `mt_supply`, `mt_batch_supply`
- `mt_tokens`, `mt_tokens_for_owner`
- `mt_metadata_contract`, `mt_metadata_base_by_metadata_id`,
  `mt_metadata_base_by_token_id`, `mt_metadata_token_all`,
  `mt_metadata_token_by_token_id`, `mt_tokens_base_metadata_all`

The transfer entry points (`mt_transfer`, `mt_batch_transfer`, `mt_transfer_call`,
`mt_batch_transfer_call`) always panic with `role NFTs are non-transferable`. Approvals are
not implemented because memberships are not transferable.

## Events

Mints and burns emit NEP-297 `nep245` events:

```json
EVENT_JSON:{"standard":"nep245","version":"1.0.0","event":"mt_mint","data":[{"owner_id":"alice.testnet","token_ids":["2355721957:alice.testnet"],"amounts":["1"]}]}
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
  --accountId hr.testnet

near view rbac.testnet has_role '{"account_id":"alice.testnet","role":"admin"}'
near view rbac.testnet roles_of '{"account_id":"alice.testnet"}'

near call rbac.testnet claim \
  '{"account_id":"alice.testnet"}' \
  --accountId alice.testnet \
  --gas 100000000000000
```

## Limitations

- Not audited.
- 32-bit role ids; collisions are detected at configuration time.
- Maximum 32 roles per account.
- Roles cannot be renamed or deleted.
- Burning forfeits unclaimed salary.
- Payouts require the contract to hold the salary token and the member to be registered with
  that token contract. There is no retry queue.
- The callback reads at most 1 KiB of the promise result.
- `claim` loops over the member's roles in one call.

See the root [README](../../README.md#limitations) for the project-wide limitations.
