# RBAC Factory

A NEAR factory contract that deploys [`rbac-nft`](../rbac-nft) contracts as subaccounts and
keeps an on-chain registry for discovery.

## Why a factory?

Deploying a contract normally requires a funded account, a wasm file, and a CLI. The factory
turns that into one wallet transaction:

1. The factory stores the rbac-nft wasm once (uploaded by the factory owner).
2. A user calls `deploy` with a subaccount label, the desired contract owner, salary token, and
   payout interval, attaching the required deposit and gas.
3. The factory creates `{name}.{factory}`, deploys the wasm, funds the account, and calls the
   child's `new` initializer.
4. A private callback records the child in the registry only after initialization succeeds.

The web frontend uses the registry to find contracts without a separate indexer, then reads the
current owner and roles directly from each child contract.

## Accounts and keys

The factory does **not** add a full-access key to the child. That makes factory-deployed
contracts immutable at the protocol level: the configured `owner_id` controls all contract
methods, but nobody can upgrade or delete the child without existing keys. This is deliberate
for a hackathon deployment and keeps the factory from becoming a permanent upgrade authority.
See [Limitations](#limitations).

## API

| Method | Auth | Description |
| --- | --- | --- |
| `new(owner_id)` | init | Creates the factory. |
| `owner_id() -> AccountId` | view | Factory owner. |
| `set_owner(new_owner_id)` | factory owner | Transfers factory ownership. |
| `set_code(code_base64)` | factory owner, payable | Stores/replaces the child wasm. Deposit must cover storage. |
| `code_len() -> U64` | view | Stored wasm length. |
| `code_hash() -> String` | view | SHA-256 of the stored wasm as hex. |
| `required_deploy_deposit() -> U128` | view | Deposit needed for one deployment at current storage prices. |
| `deploy(name, owner_id, salary_token, payout_interval_ns) -> Promise` | payable | Deploys and initializes a child. Attach at least 150 Tgas. |
| `on_deploy(...) -> bool` | private callback | Registers the child on success. |
| `contract_count() -> u64` | view | Number of registered children. |
| `get_contract(account_id) -> Option<ContractRecord>` | view | One registry entry. |
| `get_contracts(from_index?, limit?) -> Vec<ContractRecord>` | view | Paginated registry (default 50, max 100). |
| `contracts_of_owner(owner_id, ...)` | view | Children deployed for an owner (deploy-time owner). |
| `contracts_of_deployer(deployer_id, ...)` | view | Children paid for by an account. |
| `withdraw(beneficiary_id, amount)` | factory owner | Withdraws surplus balance above storage stake. |

`ContractRecord`:

```json
{
  "account_id": "acme.factory.testnet",
  "name": "acme",
  "owner_id": "dao.testnet",
  "salary_token": "usdc.testnet",
  "payout_interval_ns": "604800000000000",
  "deployer_id": "alice.testnet",
  "deployed_at_ns": "1710000000000000000"
}
```

`owner_id` is the owner supplied at deployment. The child can later call `set_owner`, so clients
that need the current owner must query the child's `owner_id()` view. The web frontend does
exactly that and treats the registry as a discovery hint, not as an ownership oracle.

## Deploying the factory

Build the child wasm first:

```bash
./scripts/build-contract.sh
```

Then deploy the factory and upload the wasm:

```bash
near deploy factory.testnet res/rbac_factory.wasm \
  --initFunction new \
  --initArgs '{"owner_id":"dao.testnet"}'

# base64 encode and upload; the deposit must cover ~5.3 NEAR for a 527 KiB wasm
near call factory.testnet set_code \
  "{\"code\":\"$(base64 -w0 res/rbac_nft.wasm)\"}" \
  --accountId dao.testnet \
  --deposit 6 \
  --gas 300000000000000
```

On macOS use `base64 -i res/rbac_nft.wasm` instead of `base64 -w0`.

Check the deposit required per child:

```bash
near view factory.testnet required_deploy_deposit
```

The value grows with the wasm size and the network storage price. For the current artifact it is
roughly **5.3 NEAR** per child, because every child stores its own copy of the wasm.

Deploy a child:

```bash
near call factory.testnet deploy \
  '{
    "name":"acme",
    "owner_id":"dao.testnet",
    "salary_token":"usdc.testnet",
    "payout_interval_ns":"604800000000000"
  }' \
  --accountId alice.testnet \
  --deposit 6 \
  --gas 300000000000000
```

## Events

Deployments emit a NEP-297 custom event:

```text
EVENT_JSON:{"standard":"rbac-factory","version":"1.0.0","event":"deploy","data":[{...ContractRecord...}]}
```

Failed initializations emit `deploy_failed` with the target account and a reason. Failed
deployments are not registered.

## Limitations

- A failed child `new` call leaves an uninitialized account at the target id. The factory has no
  key for the new account and therefore cannot delete or reinitialize it. The child is not added
  to the registry; retry with a different `name`.
- Factory-deployed children have no protocol-level access keys. They are immutable. If you need
  upgradeability, deploy manually or extend the factory to add a caller-provided key (and accept
  the associated custody risk).
- The registry is append-only. It does not track child ownership changes or contract deletion.
  Clients must verify `owner_id()` on the child.
- Every child stores a full copy of the wasm. This is simple and independent, but storage-heavy;
  a global-contract deployment pattern would be cheaper.
- Discovery reads the whole registry. For very large registries, run an indexer over the
  `deploy` events instead of paginating all records in the browser.
