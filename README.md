# NEAR RBAC NFT

Non-transferable **NEP-245 role NFTs**, on-chain **role-gated access checks**, and
**NEP-141 salary streaming** — plus a **factory** for one-transaction deployments, a Rust
**OpenID Connect provider** that turns on-chain roles into off-chain service access, and an
Astro web app for deploying and managing contracts.

The contract is designed to be owned by a Sputnik DAO or Trezu account. Each role is
represented by a non-transferable NFT membership. Role owners can mint and burn memberships,
other contracts can query `has_role`, and members can claim salary in any NEP-141 token.

```
                     owns / governs
   DAO or Trezu ───────────────────────► rbac-nft contract
                                              │
                         role NFTs (NEP-245)  │  salary (NEP-141 ft_transfer)
                                              ▼
                                        salary token

   browser ── wallet ──► rbac-factory ──deploy──► rbac-nft
      │                     │ registry
      └────── reads ────────┘

   NEAR wallet ── NEP-413 login ──► rbac-oidc ── JSON-RPC `roles_of` ──► rbac-nft
                                       │
                                       ▼
                               OIDC client / app
```

> **Status:** hackathon project. The code is well tested but has not been audited. See
> [Security model](#security-model) and [Limitations](#limitations) before using it with real
> funds.

## Contents

- [Why role NFTs?](#why-role-nfts)
- [Repository layout](#repository-layout)
- [Quick start](#quick-start)
- [Role and token identifiers](#role-and-token-identifiers)
- [Contract API](#contract-api)
- [Salary model](#salary-model)
- [Using roles from another contract](#using-roles-from-another-contract)
- [Factory and web app](#factory-and-web-app)
- [OIDC provider](#oidc-provider)
- [Security model](#security-model)
- [Limitations](#limitations)
- [Testing](#testing)
- [License](#license)

## Why role NFTs?

- **Composable:** roles are NEP-245 tokens, so wallets, indexers, and marketplaces can display
  them without a custom contract interface.
- **Non-transferable:** a membership cannot be sold or moved. Burning it removes access
  immediately; only the contract owner or the role owner can burn it.
- **Delegated administration:** each role can have its own owner (for example an HR account),
  while the DAO keeps ultimate control.
- **On-chain checks:** other contracts can call `has_role` / `has_role_id` before performing a
  gated action.
- **Salary attached:** a role can pay a fixed amount per interval in a NEP-141 token, with
  claim timestamps managed by the contract.
- **Off-chain SSO:** the included OIDC provider authenticates a NEAR account with NEP-413 and
  authorizes it using the live on-chain roles.

## Repository layout

```
.
├── Cargo.toml                     # Rust workspace
├── crates/rbac-common/            # Stable role ids and token-id encoding, shared by contract + service
├── contracts/rbac-nft/            # NEP-245 role NFT + salary contract
│   └── src/
│       ├── lib.rs                 # Contract methods
│       ├── metadata.rs            # NEP-245 metadata types
│       ├── events.rs              # NEP-297 mt_mint / mt_burn events
│       └── tests/                 # 76 unit tests
├── contracts/rbac-factory/        # Factory, child wasm storage, deployment registry
│   └── src/                       # deploy/set_code/registry + 24 tests
├── services/oidc/                 # OIDC provider (axum)
│   ├── src/                       # Config, NEP-413, RPC role source, JWT, handlers
│   ├── tests/oidc_flow.rs         # 14 end-to-end HTTP tests
│   ├── config.example.toml        # Annotated example configuration
│   └── Dockerfile
├── services/web/                  # Astro frontend (static, wallet-selector, no indexer)
│   ├── src/                       # Page, orchestration, RPC/contract/favorites helpers
│   └── tests/                     # 32 Vitest unit tests
├── scripts/build-contract.sh      # Build both wasm artifacts into res/
└── .github/workflows/ci.yml
```

## Quick start

### Prerequisites

- Rust **1.95+** (the current `near-sdk` dependency graph requires it)
- The `wasm32-unknown-unknown` target:
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- [near-cli](https://github.com/near/near-cli) or
  [near-cli-rs](https://github.com/near/near-cli-rs) for deployment
- Optional: [`cargo-near`](https://github.com/near/cargo-near) for reproducible builds
- Node.js **22.23.4+** and npm 9.6.5+ for the web app

### Build and test

```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

cd services/web
npm install
npm test
npm run check
npm run build
```

### Build the contracts

```bash
./scripts/build-contract.sh
# or individually:
cargo build -p rbac-nft -p rbac-factory --target wasm32-unknown-unknown --release
```

The artifacts are `res/rbac_nft.wasm` and `res/rbac_factory.wasm` (the script also leaves
them under `target/wasm32-unknown-unknown/release/`).

### Deploy

Deploy with an initial owner, a salary token, and a payout interval. The interval is in
**nanoseconds**:

| Period | `payout_interval_ns` |
| --- | --- |
| 1 second | `1000000000` |
| 1 hour | `3600000000000` |
| 1 day | `86400000000000` |
| 1 week | `604800000000000` |
| 30 days | `2592000000000000` |

Legacy `near-cli` example (adapt the syntax if you use `near-cli-rs`):

```bash
near deploy \
  --accountId rbac.testnet \
  --wasmFile res/rbac_nft.wasm \
  --initFunction new \
  --initArgs '{"owner_id":"dao.testnet","salary_token":"usdc.testnet","payout_interval_ns":"604800000000000"}'
```

`owner_id` should be the DAO/Trezu account. If you deploy from another account and later
hand the contract to the DAO, call `set_owner` first while you still control the deployer.

### Deploy through the factory (recommended)

The factory deploys `{name}.{factory}` in one wallet transaction and keeps a registry for
discovery. Deploy the factory, upload the child wasm once, then let users deploy children:

```bash
near deploy factory.testnet res/rbac_factory.wasm \
  --initFunction new \
  --initArgs '{"owner_id":"dao.testnet"}'

near call factory.testnet set_code \
  "{\"code\":\"$(base64 -w0 res/rbac_nft.wasm)\"}" \
  --accountId dao.testnet \
  --deposit 6 \
  --gas 300000000000000

near view factory.testnet required_deploy_deposit
```

Each child stores its own copy of the wasm, so the deposit is roughly **5–6 NEAR** and is not a
fee: the factory forwards it to the child. A child deployed by the factory has no protocol-level
access key and is therefore immutable. See
[`contracts/rbac-factory/README.md`](contracts/rbac-factory/README.md) for the full API and
caveats.

### Run the web app

```bash
cd services/web
cp .env.example .env
# set PUBLIC_FACTORY_CONTRACT_ID and the network/RPC values
npm install
npm run dev
```

The app deploys contracts, discovers contracts owned by an address (with local favorites),
shows roles held by an address with payout progress and claim buttons, and exposes owner/role
owner administration controls. See [`services/web/README.md`](services/web/README.md).

### Configure a role and grant it

```bash
# Create/update a role. Salary is in the salary token's smallest unit.
near call rbac.testnet set_role_salary \
  '{"role":"admin","salary":"1000000000000000000000000"}' \
  --accountId dao.testnet

# Optional: delegate role administration to an operational account.
near call rbac.testnet set_role_owner \
  '{"role":"admin","owner":"hr.testnet"}' \
  --accountId dao.testnet

# Mint the role membership. The role must already be configured.
near call rbac.testnet mint_role \
  '{"account_id":"alice.testnet","role":"admin"}' \
  --accountId hr.testnet
```

Read it back:

```bash
near view rbac.testnet has_role '{"account_id":"alice.testnet","role":"admin"}'
near view rbac.testnet roles_of '{"account_id":"alice.testnet"}'
near view rbac.testnet get_role '{"role":"admin"}'
```

Claim salary (anyone may trigger a claim; the payout always goes to the member):

```bash
near call rbac.testnet claim \
  '{"account_id":"alice.testnet"}' \
  --accountId alice.testnet \
  --gas 100000000000000
```

If the contract owner is a Sputnik DAO, these calls are normally made as DAO proposals and
executed by the DAO account.

## Role and token identifiers

Role ids are stable across the contract, the OIDC service, and indexers:

```text
role_id          = u32::from_be_bytes(SHA-256(role_name)[0..4])
token_id         = "{role_id}:{account_id}"
base_metadata_id = "{role_id}"          (decimal string)
```

NEAR account ids cannot contain `:`, so the token-id encoding is unambiguous.

Documented test vectors:

| Role name | `role_id` |
| --- | ---: |
| `admin` | `2355721957` |
| `employee` | `802947447` |
| `manager` | `1860478057` |
| `hr.manager` | `1475994381` |
| `Treasury` | `1793088973` |

Role names are case-sensitive (`Admin` and `admin` are different roles), must be non-empty,
at most 64 bytes, and must not have surrounding whitespace or control characters. A 32-bit
id can collide; the contract rejects a second role name that hashes to an existing id.

JavaScript helper:

```js
const crypto = require("node:crypto");

function roleId(name) {
  return crypto.createHash("sha256").update(name, "utf8").digest().readUInt32BE(0);
}

function tokenId(name, accountId) {
  return `${roleId(name)}:${accountId}`;
}
```

## Contract API

All amounts are `U128` and serialize as decimal strings. Timestamps are nanoseconds.

### Initialization and configuration

| Method | Auth | Description |
| --- | --- | --- |
| `new(owner_id, salary_token, payout_interval_ns)` | init | Creates the contract. |
| `owner_id() -> AccountId` | view | Current contract owner. |
| `salary_token() -> AccountId` | view | NEP-141 token used for payouts. |
| `payout_interval_ns() -> U64` | view | Global payout interval. |
| `set_owner(new_owner_id)` | contract owner | Transfers contract ownership. |
| `set_payout_config(salary_token, payout_interval_ns)` | contract owner | Changes the salary token and/or interval. |
| `set_role_salary(role, salary) -> RoleView` | contract owner | Creates a role or updates its salary. |
| `set_role_owner(role, owner: Option<AccountId>) -> RoleView` | contract owner or effective role owner | Delegates/clears role administration. `None` falls back to the contract owner. |
| `get_role(role) -> Option<RoleView>` | view | Role by name. |
| `get_role_by_id(role_id) -> Option<RoleView>` | view | Role by id. |
| `get_roles(from_index?, limit?) -> Vec<RoleView>` | view | Paginated roles; default limit 50, maximum 100. |
| `role_count() -> u32` | view | Number of configured roles. |

`RoleView` contains `role_id`, `name`, effective `owner`, `configured_owner`, `salary`, and
`holders`.

### Membership

| Method | Auth | Description |
| --- | --- | --- |
| `mint_role(account_id, role) -> String` | contract owner or role owner | Mints a membership; returns the token id. Starts salary accrual at the current block timestamp. |
| `burn_role(account_id, role)` | contract owner or role owner | Burns a membership. Access stops immediately and unclaimed salary is discarded. |
| `has_role(account_id, role) -> bool` | view | Role-gated access check by name. |
| `has_role_id(account_id, role_id) -> bool` | view | Role-gated access check by id. |
| `roles_of(account_id) -> Vec<String>` | view | Sorted role names held by an account. |
| `role_ids_of(account_id) -> Vec<u32>` | view | Sorted role ids held by an account. |
| `members_of(role) -> Vec<AccountId>` | view | Sorted members of a role. |

Rules:

- A role must be configured with `set_role_salary` before it can be minted (a salary of `0`
  is allowed).
- An account can hold a given role at most once.
- An account can hold at most **32 roles**. This cap keeps `claim` inside the block gas limit.
- Role ownership defaults to the contract owner. The role owner can transfer or clear its own
  role ownership; the contract owner can always manage any role.

### Salary

| Method | Auth | Description |
| --- | --- | --- |
| `claim(account_id) -> PromiseOrValue<U128>` | anyone | Claims all full intervals accrued by the account. |
| `claim_role(account_id, role) -> PromiseOrValue<U128>` | anyone | Claims one role only. |
| `claimable(account_id) -> U128` | view | Amount currently claimable. |
| `claimable_role(account_id, role) -> U128` | view | Amount currently claimable for one role. |
| `last_claim(account_id, role) -> Option<U64>` | view | Last claim timestamp for a role. |
| `on_claim_transfer(...) -> bool` | private callback | Restores claim timestamps if the NEP-141 transfer failed. |

### NEP-245

Read/enumeration/metadata methods:

- `mt_token`, `mt_balance_of`, `mt_batch_balance_of`, `mt_supply`, `mt_batch_supply`
- `mt_tokens`, `mt_tokens_for_owner`
- `mt_metadata_contract`, `mt_metadata_base_by_metadata_id`,
  `mt_metadata_base_by_token_id`, `mt_metadata_token_all`,
  `mt_metadata_token_by_token_id`, `mt_tokens_base_metadata_all`

The transfer methods (`mt_transfer`, `mt_batch_transfer`, `mt_transfer_call`,
`mt_batch_transfer_call`) are implemented but **always panic** with
`"role NFTs are non-transferable"`. Approvals are intentionally not implemented because
memberships cannot be transferred.

### Events

Membership changes emit NEP-297 `nep245` events:

```json
EVENT_JSON:{"standard":"nep245","version":"1.0.0","event":"mt_mint","data":[{"owner_id":"alice.testnet","token_ids":["2355721957:alice.testnet"],"amounts":["1"]}]}
```

Burns are the same shape with event `mt_burn`; they include `authorized_id` when someone
other than the member triggered the burn.

## Salary model

- The payout interval is global; each role has its own salary per interval.
- Minting starts accrual at the current block timestamp. Tokens minted later do not
  retroactively earn salary.
- Only **full intervals** are paid. A claim advances each role's timestamp by
  `periods * payout_interval_ns`; the partial remainder stays for the next claim.
- Example with a 1-day interval and a 10-token salary, minted at day 0:
  - claim at day 2.5 pays `2 × 10 = 20` and sets the timestamp to day 2;
  - claim at day 3.5 pays `1 × 10 = 10` and sets the timestamp to day 3.
- Anyone can trigger `claim`; the transfer always goes to the member.
- If nothing is due, `claim` returns `0` without creating a transfer.
- Burning a role discards its unclaimed salary. Claim first if that matters.
- Changing a role salary or the global interval applies retroactively to all **unclaimed**
  elapsed time. Claim before reconfiguring payroll if that is not desired.
- Payouts call `ft_transfer` on the salary token with `1 yoctoNEAR` attached and a 30 Tgas
  static gas limit. The contract must hold enough of the token.
- On a failed transfer, the private callback restores the previous claim timestamps so the
  member can retry. The member must be registered with the token contract, otherwise every
  claim for that token will fail. If the callback itself runs out of gas or cannot read the
  promise result, timestamps may not be restored (see [Limitations](#limitations)).

## Using roles from another contract

`has_role` and `has_role_id` are view methods. A contract cannot consume a view result
synchronously; use an asynchronous cross-contract call and continue in a callback:

```rust
use near_sdk::{ext_contract, AccountId, Gas};

#[ext_contract(ext_rbac)]
trait Rbac {
    fn has_role(&self, account_id: AccountId, role: String) -> bool;
}

// Start the check. `rbac_account` is the deployed rbac-nft contract.
ext_rbac::ext(rbac_account)
    .with_static_gas(Gas::from_tgas(10))
    .has_role(account_id, "admin".to_owned())
    .then(
        Self::ext(near_sdk::env::current_account_id())
            .with_static_gas(Gas::from_tgas(10))
            .on_role_checked(/* ... */),
    );
```

Alternatively, call `has_role_id` with a role id computed once and hard-coded or supplied by
the caller.

Off-chain checks can read `roles_of` or `has_role` through NEAR JSON-RPC:

```bash
ARGS=$(printf '%s' '{"account_id":"alice.testnet"}' | base64 -w0)
curl -s https://rpc.testnet.near.org \
  -H 'content-type: application/json' \
  -d "{
    \"jsonrpc\":\"2.0\",
    \"id\":\"roles\",
    \"method\":\"query\",
    \"params\":{
      \"request_type\":\"call\",
      \"finality\":\"final\",
      \"account_id\":\"rbac.testnet\",
      \"method_name\":\"roles_of\",
      \"args_base64\":\"$ARGS\"
    }
  }"
```

The contract result is a JSON byte array in `result.result`; decode it with
`JSON.parse(Buffer.from(result.result).toString())` in JavaScript.

## Factory and web app

### Factory

[`contracts/rbac-factory`](contracts/rbac-factory) lets anyone create an initialized
`rbac-nft` as `{name}.{factory}` with a single transaction:

1. The factory owner uploads the `rbac-nft` wasm once with
   `set_code(code_base64)`, attaching enough deposit for storage.
2. A deployer calls
   `deploy(name, owner_id, salary_token, payout_interval_ns)` with at least
   `required_deploy_deposit()` and enough gas (150 Tgas minimum; the web app sends 300).
3. The factory creates the account, deploys the wasm, funds it, and calls `new`.
4. A private callback records the child only after initialization succeeds and emits a
   `rbac-factory` NEP-297 `deploy` event.

Registry views: `contract_count`, `get_contract`, `get_contracts`, `contracts_of_owner`, and
`contracts_of_deployer`. `contracts_of_owner` uses the **deploy-time** owner; clients that need
the current owner must query the child's `owner_id()`.

Factory-deployed children have no protocol-level access key, so they cannot be upgraded or
deleted. A failed initialization leaves an orphan account at the target id; retry with another
name. The registry is append-only and does not track ownership transfers or deletion.

### Web app

[`services/web`](services/web) is a static Astro app with vanilla TypeScript and
wallet-selector. It has three views:

| View | Behavior |
| --- | --- |
| **Deploy** | Shows factory status and the required deposit, validates the subaccount/owner/token/interval, and calls `deploy` through the connected wallet. |
| **Owned contracts** | Scans the full registry, verifies each child's live `owner_id()`, filters by the scanned address, and supports local favorites to suppress spam. |
| **My roles** | Scans the registry with `roles_of`, then reads role salary, `last_claim`, `claimable_role`, and renders a payout interval progress bar and claim button per membership. |

Contract owners get role creation/salary, role-owner delegation, payroll configuration, and
ownership transfer controls. Contract or role owners get mint/burn controls. All reads go
directly to NEAR JSON-RPC; no indexer is required. The registry is a discovery hint only: every
displayed contract is re-checked on-chain, and favorites are scoped by network and account in
`localStorage`.

## OIDC provider

The `rbac-oidc` service is a small OpenID Connect provider that authenticates a NEAR account
with **NEP-413** and authorizes it with **live on-chain roles**.

### Endpoints

| Endpoint | Description |
| --- | --- |
| `GET /.well-known/openid-configuration` | OIDC discovery. |
| `GET /.well-known/oauth-authorization-server` | OAuth authorization-server metadata (same document). |
| `GET /jwks.json` | RSA public signing key as a JWK set. |
| `GET /authorize` | Validates the client, redirect URI, scopes, and PKCE; renders the login page. |
| `POST /login` | Verifies the NEP-413 signature, access-key ownership, and required roles; redirects with a one-time code. |
| `POST /token` | Exchanges an authorization code (PKCE + client auth) for an access token and ID token. |
| `GET/POST /userinfo` | Returns `sub`, `preferred_username`, and live roles for a bearer token. |
| `POST /introspect` | RFC 7662-style token introspection with live role checks. |
| `POST /logout` | Revokes a session by access token or ID token. |
| `GET /healthz` | Liveness check. |

### Flow

1. The client generates a PKCE verifier and S256 challenge and redirects the user to
   `/authorize` with `response_type=code`, `client_id`, an exact registered `redirect_uri`,
   `scope`, optional `state`/`nonce`, `code_challenge`, and
   `code_challenge_method=S256`.
2. The provider shows a login page containing a NEP-413 message and nonce. The user signs it
   with a wallet key (or pastes a signature). The page posts `account_id`, `public_key`, and
   `signature` to `/login`.
3. `/login` verifies the signature over the NEP-413 payload, checks that the public key is an
   access key of the claimed account (unless `verify_access_key=false`), and checks the
   client's `required_roles` against `roles_of`.
4. On success, the browser is redirected to the registered redirect URI with a one-time
   `code` (and the original `state`).
5. The client posts the code, redirect URI, client id/secret, and PKCE verifier to `/token`.
   The provider consumes the code and returns an RS256 `access_token` and `id_token`.
6. The client validates the JWT with `/jwks.json`, then calls `/userinfo` or `/introspect`
   for live roles.

Example token exchange:

```bash
curl -sS -X POST https://auth.example.com/token \
  -H 'content-type: application/x-www-form-urlencoded' \
  --data-urlencode grant_type=authorization_code \
  --data-urlencode code="$CODE" \
  --data-urlencode redirect_uri=https://app.example.com/callback \
  --data-urlencode client_id=demo-app \
  --data-urlencode code_verifier="$VERIFIER"
```

```json
{
  "access_token": "eyJ...",
  "token_type": "Bearer",
  "expires_in": 900,
  "id_token": "eyJ...",
  "scope": "openid profile roles"
}
```

```bash
curl -sS -H "Authorization: Bearer $ACCESS_TOKEN" https://auth.example.com/userinfo
curl -sS -X POST https://auth.example.com/introspect --data-urlencode token="$ACCESS_TOKEN"
curl -sS -X POST https://auth.example.com/logout --data-urlencode token="$ACCESS_TOKEN"
```

### Claims

| Token | Claims |
| --- | --- |
| Access token | `iss`, `sub`, `aud`, `exp`, `iat`, `jti` (session id), `client_id`, `scope`, `roles`, `token_use="access"` |
| ID token | `iss`, `sub`, `aud`, `exp`, `iat`, `auth_time`, `sid`, optional `nonce`, `preferred_username`, `roles`, `token_use="id"` |

The `aud` claim is the OIDC client id. The access token's `jti` is the session id, so
logout/introspection can revoke it. `/userinfo` refreshes roles from the chain on every
call. If a session no longer satisfies the client's `required_roles`, the provider revokes
it and returns `401`.

### Configuration and deployment

Start from [`services/oidc/config.example.toml`](services/oidc/config.example.toml). The
main fields are:

| Field | Default | Description |
| --- | --- | --- |
| `issuer` | required | Public HTTPS base URL; used as `iss` and discovery prefix. No trailing slash. |
| `listen` | `127.0.0.1:8080` | Bind address. |
| `rpc_url` | required | NEAR JSON-RPC endpoint for `roles_of` and access-key checks. |
| `contract_id` | required | Deployed `rbac-nft` account. |
| `clients` | required | One or more registered OIDC clients. |
| `nep413_recipient` | issuer host | Recipient string bound into the signed message. |
| `nep413_callback_url` | unset | Optional callback URL bound into the signed message. |
| `access_token_ttl_secs` | `900` | Access-token/session lifetime. |
| `id_token_ttl_secs` | `900` | ID-token lifetime. |
| `authorization_code_ttl_secs` | `300` | One-time code lifetime. |
| `pending_login_ttl_secs` | `300` | Login page / challenge lifetime. |
| `signing_key_path` | unset | PKCS#8 PEM RSA key. If unset, an ephemeral key is generated. |
| `verify_access_key` | `true` | Require the NEP-413 key to be an access key of the account. |
| `allowed_scopes` | `openid profile roles` | Global scope allow-list. |

Each client supports `client_id`, `name`, `redirect_uris`, optional `client_secret`
(confidential client), `required_roles`, and optional per-client `allowed_scopes`.

Generate a signing key and run:

```bash
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out services/oidc/signing-key.pem
chmod 600 services/oidc/signing-key.pem

cp services/oidc/config.example.toml services/oidc/config.toml
# edit issuer, rpc_url, contract_id, clients, signing_key_path

cargo run -p rbac-oidc --release -- services/oidc/config.toml
```

`RBAC_OIDC_CONFIG` can also point at the config file. Put the service behind a TLS reverse
proxy so the configured `issuer` is reachable over HTTPS. The included
[`Dockerfile`](services/oidc/Dockerfile) builds a non-root runtime image from the repository
root:

```bash
docker build -f services/oidc/Dockerfile -t rbac-oidc .
docker run --rm -p 8080:8080 \
  -v "$PWD/services/oidc/config.toml:/app/config.toml:ro" \
  -v "$PWD/services/oidc/signing-key.pem:/app/signing-key.pem:ro" \
  rbac-oidc
```

See [`services/oidc/README.md`](services/oidc/README.md) for a more detailed service guide.

## Security model

**Contract**

- Only the contract owner can change ownership, salary configuration, payout configuration,
  and role salaries.
- Only the contract owner or the effective role owner can mint/burn a role or change that
  role's owner.
- Role NFTs cannot be transferred; transfer entry points panic.
- Token ids bind the role id and account id, and the contract checks token ownership before
  returning a role.
- Salary transfers use a private callback; only the contract itself can call it.
- Claim arithmetic uses checked multiplication/addition and integer interval math.

**Factory and web app**

- Only the factory owner can upload/replace the child wasm or withdraw surplus balance.
- The factory does not add a key to children, so a deployment cannot be upgraded or deleted by
  the factory. The configured owner controls contract methods.
- The registry is not an ownership oracle. The web app verifies `owner_id()` on the child before
  showing a contract as owned and never grants UI permissions from registry data alone.
- Wallet writes are signed by the user's wallet; the app never handles private keys.
- Favorites are stored locally and only filter display. They do not affect on-chain state.
- RPC read results are trusted, as with any dApp. Use a reputable endpoint.

**OIDC**

- NEP-413 signature verification binds the message, a 256-bit nonce, the recipient, and the
  optional callback URL.
- By default, the signing key must also be a current access key of the claimed account. This
  is what prevents someone from signing with their own key while claiming a privileged
  account id. Keep `verify_access_key=true`.
- PKCE S256 is mandatory. Code challenges and verifiers are length/charset validated.
- Redirect URIs are matched exactly against the registered value.
- Authorization codes are 256-bit random values, single-use, short-lived, and bound to the
  client id, redirect URI, PKCE challenge, account, and roles. A code is consumed even if the
  exchange later fails.
- Confidential client secrets are compared in constant time. Basic and POST authentication
  are supported.
- JWTs are RS256, validated for issuer and expiry, and published through `/jwks.json`.
- Sessions are referenced by the access token's `jti`; logout revokes them.
- `/userinfo` and `/introspect` re-read roles from the chain and revoke a session when the
  client's required roles disappear.

**Factory and web app**

- Only the factory owner can upload/replace the child wasm or withdraw surplus balance.
- The factory does not add a key to children, so a deployment cannot be upgraded or deleted by
  the factory. The configured owner controls contract methods.
- The registry is not an ownership oracle. The web app verifies `owner_id()` on the child before
  showing a contract as owned and never grants UI permissions from registry data alone.
- Wallet writes are signed by the user's wallet; the app never handles private keys.
- Favorites are stored locally and only filter display. They do not affect on-chain state.
- RPC read results are trusted, as with any dApp. Use a reputable endpoint.

**Operational**

- Use HTTPS for the issuer; never expose the signing key.
- Keep the contract owner as the DAO/Trezu account, not a personal key.
- Treat the RPC endpoint as trusted infrastructure: the OIDC service believes its
  `roles_of` and `view_access_key` answers. Use a dedicated/redundant RPC provider for
  production.
- Back up the signing key. Without it, all issued tokens become unverifiable after a restart.
- Run a single OIDC replica with the in-memory store, or add a shared persistent store before
  scaling horizontally.

## Limitations

This is deliberately a hackathon-scale implementation. Known limitations:

**Contract**

- Not audited. Use with real funds only after review.
- Role ids are 32-bit hashes. Collisions are detected when configuring a role, but there is no
  global registry across contracts.
- An account can hold at most 32 roles.
- Roles cannot be renamed or deleted. Salary/interval changes are retroactive to unclaimed
  time.
- Burning a role forfeits its unclaimed salary.
- Salary payouts depend on the token contract and the contract's balance. There is no
  retry queue; a failed transfer restores timestamps only if the callback executes and can
  read the result.
- The callback reads at most 1 KiB of the promise result. A token returning a larger payload
  could be treated as a failed transfer.
- NEP-245 transfer methods intentionally panic and approval methods are absent, so this is
  not a marketplace-compatible collection.
- `claim` iterates all roles held by an account in one call; the 32-role cap keeps this
  bounded but does not guarantee gas availability for every token implementation.

**Factory and web app**

- A failed child `new` call leaves an uninitialized account. The factory cannot delete it and
  cannot retry the same name.
- Children deployed through the factory are immutable because they have no access keys. Plan
  for upgrades before using it for a long-lived production contract.
- Every child stores a full copy of the wasm, so deployment is storage-heavy. The required
  deposit is roughly 5–6 NEAR at current prices.
- The registry is append-only and does not track ownership changes or deletion.
- The web app scans the whole registry and makes one or more RPC reads per child. This is fine
  for small registries but grows linearly; an indexer is the next step for scale.
- Reads have a 20-second timeout. A failed child read is treated as not-owned/no-roles for that
  contract instead of failing the whole scan.
- Token metadata is best-effort; missing metadata falls back to raw units.
- The frontend is not audited and does not pin/verify the factory wasm hash.

**OIDC**

- The pending-login/code/session store is process-local and in-memory. Restarting loses
  sessions; multiple replicas do not share state. Expired entries are not proactively
  garbage-collected, so abandoned logins and old sessions remain in memory until restart.
- No refresh tokens, dynamic client registration, consent screen, device flow, token
  exchange, DPoP, or CORS.
- One RSA signing key, loaded at startup. There is no key rotation endpoint or multi-key
  JWKS.
- Access tokens are self-contained JWTs. A client that only validates the JWT locally will
  not notice a logout or role revocation until the token expires; clients that call
  `/userinfo` or `/introspect` see it immediately.
- `/introspect` allows unauthenticated requests for public-client tokens; tokens issued to
  confidential clients require matching client credentials. Protect the endpoint at the
  network layer if you need stronger isolation.
- OAuth errors from `/authorize` are returned as JSON rather than redirected to the client's
  redirect URI. `/token` and `/login` follow the JSON error style.
- The built-in login page relies on a NEP-413 wallet (`window.nearWallet` or wallet-selector)
  or manual signature entry. It is a developer-friendly login, not a polished consent UI.
- No rate limiting, audit logging, or brute-force protection is built in.
- `verify_access_key=false` is unsafe outside local experiments; it removes the binding
  between the signing key and the claimed account.

## Testing

```bash
cargo test --workspace
```

The Rust workspace currently has **152 tests**, and the web app has **32 Vitest tests**
(**184 total**):

- `crates/rbac-common`: 6 tests covering role hashing, token-id encoding, documented vectors,
  and role-name validation.
- `contracts/rbac-nft`: 76 tests covering initialization/admin permissions, role creation,
  delegation, mint/burn, membership queries, NEP-245 reads/enumeration/metadata, salary
  accrual, partial intervals, claim callbacks, and the non-transferable entry points.
- `contracts/rbac-factory`: 24 tests covering code upload/storage, name and gas/deposit
  validation, child creation receipts, callback registration/failure, duplicate protection,
  registry pagination, owner/deployer indexes, and owner-only administration.
- `services/oidc`: 32 unit tests covering config validation (including the example config),
  RSA/JWKS, NEP-413 serialization and signatures, role-result decoding, and handler helpers.
- `services/oidc/tests/oidc_flow.rs`: 14 end-to-end HTTP tests covering discovery, JWKS,
  full PKCE authorization-code flow, code binding/reuse/expiry, confidential clients,
  login signature/access-key checks, required roles, live role revocation, logout,
  introspection, and client binding for introspection.
- `services/web/tests`: 32 Vitest tests covering role-id vectors and token ids, account and
  subaccount validation, amount/duration formatting and parsing, interval progress, favorites
  scoping/persistence, bounded-concurrency mapping, and role/contract permission helpers.

Lint and wasm checks:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p rbac-nft -p rbac-factory --target wasm32-unknown-unknown --release

cd services/web && npm test && npm run check && npm run build
```

> `near-sdk` refuses to compile the contract for a native target unless the
> `unit-testing` feature is enabled, so use `cargo test -p rbac-nft`,
> `cargo check -p rbac-nft --target wasm32-unknown-unknown`, or the wasm build above.
> Plain `cargo check -p rbac-nft` without a target is expected to fail.

## License

MIT. See [LICENSE](LICENSE).
