# PayRole Factory

A NEAR factory contract that deploys [`payrole-nft`](../payrole-nft) contracts as subaccounts and
keeps an on-chain registry for discovery.

## Why a factory?

Deploying a contract normally requires a funded account, a wasm file, and a CLI. The factory
turns that into one wallet transaction:

1. The factory is initialized once with the payrole-nft wasm in `new(code)`; the code is immutable.
2. A user calls `deploy` with a subaccount label, the desired contract owner, salary token, and
   payout interval, attaching the required deposit and gas.
3. The factory creates `{name}.{factory}`, deploys the wasm, funds the account, and calls the
   child's `new` initializer. The child is told the factory account id in `factory_id`.
4. A private callback records the child in the registry only after initialization succeeds.
5. When the child's `set_owner` runs, the child calls `on_owner_changed` so the factory updates
   `ContractRecord.owner_id` and `by_owner`.

The web frontend uses the registry to find contracts without a separate indexer, then reads the
current owner and roles directly from each child contract. The documented default testnet
factory is `payrole-factory.testnet`.

## Trust model

The factory is **trustless and immutable**:

- It has no owner and no admin methods.
- It cannot replace the child wasm, change the registry code path, or withdraw funds.
- `code_hash()` exposes the SHA-256 of the stored wasm so clients can pin the code they reviewed.
- It does not add a full-access key to the child. Factory-deployed contracts are immutable at the
  protocol level: the configured `owner_id` controls all contract methods, but nobody can upgrade
  or delete the child without existing keys.
- Any deposit left in the factory is permanently locked. Deployers should attach close to
  `required_deploy_deposit()`.

## API

| Method | Auth | Description |
| --- | --- | --- |
| `new(code_base64)` | init, payable | Creates the factory and stores the child wasm permanently. Deposit must cover storage. |
| `code_len() -> U64` | view | Stored wasm length. |
| `code_hash() -> String` | view | SHA-256 of the stored wasm as hex. |
| `required_deploy_deposit() -> U128` | view | Deposit needed for one deployment at current storage prices. |
| `deploy(name, owner_id, salary_token, payout_interval_ns) -> Promise` | payable | Deploys and initializes a child. Attach at least 150 Tgas. |
| `on_deploy(...) -> bool` | private callback | Registers the child on success and refunds the full deposit on failure. |
| `on_owner_changed(new_owner_id) -> bool` | registered child | Updates the child's `owner_id` and owner index after `set_owner`. |
| `contract_count() -> u64` | view | Number of registered children. |
| `get_contract(account_id) -> Option<ContractRecord>` | view | One registry entry. |
| `get_contracts(from_index?, limit?) -> Vec<ContractRecord>` | view | Paginated registry (default 50, max 100). |
| `contracts_of_owner(owner_id, ...)` | view | Children whose current registered owner is `owner_id`. |

There is no `set_owner`, `set_code`, or `withdraw`.

`ContractRecord`:

```json
{
  "account_id": "acme.payrole-factory.testnet",
  "name": "acme",
  "owner_id": "dao.testnet",
  "salary_token": "usdc.testnet",
  "payout_interval_ns": "604800000000000",
  "deployer_id": "alice.testnet",
  "deployed_at_ns": "1710000000000000000"
}
```

`owner_id` is the child's current registered owner: the factory sets it at deployment and the
child updates it through `on_owner_changed` when `set_owner` runs. The `deployer_id` stays in the
record for auditing. The notification is **best-effort**: if the child cannot call the factory
(for example, not enough gas or a factory upgrade), the child's ownership still changes and the
factory record becomes stale. Clients must query the child's live `owner_id()` before trusting
the registry, and the web app does exactly that.

`contracts_of_owner` is an advisory index, not an authorization source. It is spammable: anyone
can deploy a child with a victim's account as `owner_id`, which adds an entry to the victim's
index. The web app verifies each child's live `owner_id()` and does not grant permissions from
the registry alone. Use local favorites to hide unwanted entries.

## Deploying the factory

Build the child wasm first:

```bash
./scripts/build-contract.sh
```

Then deploy the factory with the wasm in the init call. The included script does this in one
step and refuses to reuse an existing account:

```bash
MASTER_ACCOUNT=dao.testnet ./scripts/deploy-factory.sh

# Or manually with near-cli-rs. The base64 wasm init JSON is several hundred KiB, which exceeds
# the kernel's per-argument limit, so pass it from a file:
python3 - <<'PY' > /tmp/payrole-factory-init.json
import base64, json
with open("res/payrole_nft.wasm", "rb") as wasm:
    print(json.dumps({"code": base64.b64encode(wasm.read()).decode("ascii")}))
PY
near contract deploy payrole-factory.testnet \
  use-file res/payrole_factory.wasm \
  with-init-call new file-args /tmp/payrole-factory-init.json \
  prepaid-gas '150 Tgas' \
  attached-deposit '5 NEAR' \
  network-config testnet \
  sign-with-keychain send
```

The init call needs at least **150 Tgas**: `new` decodes the base64 argument and writes the
whole child wasm into contract state, so the default 30 Tgas fails with
`Exceeded the prepaid gas`. The init deposit must cover the stored wasm (~3.3 NEAR for the
current ~320 KiB build); the script defaults to 5 NEAR. Any remainder is locked permanently because the factory has no withdrawal method. A fresh
account is funded with `INIT_DEPOSIT + 1` NEAR by default (6 NEAR) so it can attach the deposit
and pay the deploy fee; override `INIT_BALANCE` if needed.

Check the deposit required per child and the pinned hash:

```bash
near view payrole-factory.testnet required_deploy_deposit
near view payrole-factory.testnet code_hash
```

The value grows with the wasm size and the network storage price. For the current artifact it is
roughly **3.3 NEAR** per child, because every child stores its own copy of the wasm.

Deploy a child:

```bash
near call payrole-factory.testnet deploy \
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
EVENT_JSON:{"standard":"payrole-factory","version":"1.0.0","event":"deploy","data":[{...ContractRecord...}]}
```

Failed initializations emit `deploy_failed` with the target account and a reason. Failed
deployments are not registered, and the factory refunds the full attached deposit (registry
reserve plus forwarded child balance) to the deployer. The failed child receipt is rolled back
atomically, so no account is left behind and the name can be retried.

## Limitations

- A failed child `new` call is rolled back atomically: the account is not left at the target id,
  the child is not added to the registry, and the factory refunds the full attached deposit
  (registry reserve plus forwarded child balance). The same `name` can be retried.
- Factory-deployed children have no protocol-level access keys. They are immutable. If you need
  upgradeability, deploy manually or extend the factory to add a caller-provided key (and accept
  the associated custody risk).
- Ownership updates rely on the child calling `on_owner_changed`. The notification is
  best-effort: if it fails, the child's `owner_id()` is still authoritative while the factory's
  record is stale. Deletion is not tracked at all.
- `contracts_of_owner` can be spammed by deploying with someone else's account as the initial
  owner. Treat it as a discovery hint, verify `owner_id()` on the child, and hide spam locally.
- Every child stores a full copy of the wasm. This is simple and independent, but storage-heavy.
  The documented production path is a global contract by hash (NEP-591), which burns the code
  storage once and shares it across children; break-even is roughly 10 children for a ~320 KiB
  wasm. That path was not used for this deployment because the global contract burns roughly
  38 NEAR and the testnet account lacks sufficient testnet NEAR.
- Any deposit left in the factory is permanently locked because there is no withdrawal method.
- Discovery reads the whole registry. For very large registries, run an indexer over the
  `deploy` events instead of paginating all records in the browser.
