# RBAC NFT Web

An Astro + TypeScript frontend for deploying and managing [`rbac-nft`](../../contracts/rbac-nft)
contracts through the [`rbac-factory`](../../contracts/rbac-factory).

The app is fully static. It reads NEAR JSON-RPC directly and signs writes with
[`@near-wallet-selector`](https://github.com/near/wallet-selector); there is no backend or
indexer.

## What it does

- **Deploy** a new `rbac-nft` at `{name}.{factory}` in one wallet transaction. The owner can be
  any account, including a DAO or Trezu. If `PUBLIC_EXPECTED_CHILD_CODE_HASH` is set, deployment
  is blocked unless the immutable factory reports exactly that child wasm hash.
- **Owned contracts** scans the factory registry, verifies each child's live `owner_id()`, and
  shows contracts currently owned by the scanned address. Each card shows the contract's
  salary-token balance next to `accrued_liabilities()`, marked funded or underfunded. Favorites
  are stored in `localStorage` so spam contracts deployed with your account as owner can be
  hidden.
- **My roles** scans the registry for role memberships held by an address, then shows salary,
  last claim, a payout-duration progress bar, and the currently claimable amount with a claim
  button.
- Role owners can mint and burn memberships from either tab. Minting attaches the contract's
  `required_mint_deposit()`; burning pays accrued salary first and asks for confirmation.
  Contract owners get controls for role creation, role salaries, role-owner delegation, salary
  token/interval, and ownership transfer.

Discovery is registry-based instead of indexed. The registry is treated as a hint: the app
always calls `owner_id()` / `roles_of()` on the child contract before trusting it.

## Prerequisites

- Node.js **22.23.4+** (Astro 7 requirement) and npm 9.6.5+
- A deployed [`rbac-factory`](../../contracts/rbac-factory) initialized once with the `rbac-nft`
  wasm in `new(code)`. The factory has no owner and the code cannot be replaced.

## Configuration

Copy the example environment file and edit it:

```bash
cp .env.example .env
```

| Variable | Required | Description |
| --- | --- | --- |
| `PUBLIC_NEAR_NETWORK` | no | `testnet` (default) or `mainnet`. |
| `PUBLIC_NEAR_RPC_URL` | no | RPC endpoint for reads. Defaults to the public RPC for the network. |
| `PUBLIC_FACTORY_CONTRACT_ID` | yes | Factory account, e.g. `rbac-factory.testnet`. Without it, deployment and discovery are disabled and a banner is shown. |
| `PUBLIC_EXPECTED_CHILD_CODE_HASH` | no | SHA-256 of the reviewed `rbac-nft` wasm. When set, the app refuses to deploy if `code_hash()` differs. |
| `PUBLIC_EXPLORER_URL` | no | Explorer base URL. Defaults to NEARBlocks for the network. |
| `PUBLIC_SITE_URL` | no | Absolute public URL used for the canonical link and Open Graph/Twitter image tags. When unset those tags are omitted. |

These are **build-time** variables. Rebuild after changing them. A missing factory is not
defaulted on purpose: silently pointing at the wrong registry would be worse than disabling the
app.

## Develop

```bash
npm install
npm run dev        # http://localhost:4321
npm test           # Vitest unit tests
npm run check      # astro check / TypeScript
npm run build      # check + static build into dist/
npm run preview    # serve the production build
```

## Deploy

`npm run build` produces a static site in `dist/`. Host it on any static host (IPFS, Cloudflare
Pages, Netlify, S3, …) with the environment variables set at build time.

The app needs no server, but the RPC endpoint is trusted for read results. Use a reliable
provider for production.

## Usage

1. Connect a NEAR wallet. The connected account pays for deployments and signs admin calls.
2. In **Deploy**, choose a subaccount name, the contract owner (your DAO/Trezu), the NEP-141
   salary token, and the payout interval (30 days by default). The factory reports the minimum
   deposit; the app pre-fills it. The panel shows the factory's child wasm hash and marks it
   pinned when it matches `PUBLIC_EXPECTED_CHILD_CODE_HASH`. Excess deposit above the factory's
   fixed registry reserve is forwarded to the child.
3. In **Owned contracts**, scan the owner address. Favorite the contracts you care about.
4. Open a contract's **Contract administration** panel to create roles, set salaries, delegate
   role owners, change payroll configuration, or transfer ownership.
5. In **My roles**, scan a member address. Claim salary when the claimable amount is non-zero.

Minting an existing role or burning a missing role fails on-chain. Minting attaches the
contract-reported storage deposit. Burning requests `pay_pending: true`, so accrued salary is
transferred first; the confirmation explains this and a failed transfer keeps the membership.
Write actions show a pending/disabled button and report success or error in the status line.
The page ships a key/role-badge mark, a hero flow diagram, and a social preview image
(`public/og.png`) for links shared from a `PUBLIC_SITE_URL`.

## Module map

| File | Purpose |
| --- | --- |
| `src/pages/index.astro` | Page markup, branding, hero diagram, social tags. |
| `src/scripts/app.ts` | Entry point: restores the URL, binds events and starts wallet loading. |
| `src/scripts/app/state.ts` | Shared render state, runtime counters/caches, interval units. |
| `src/scripts/app/dom.ts` | Element lookup, escaping, status/notice helpers, busy buttons. |
| `src/scripts/app/views.ts` | Contract cards, role/member tables, admin forms, rendering. |
| `src/scripts/app/intervals.ts` | Payout interval formatting and parsing. |
| `src/scripts/app/actions.ts` | Registry scans, wallet actions, deploy/mint/burn/claim/admin calls. |
| `src/scripts/app/events.ts` | DOM event wiring and per-form busy state. |
| `src/lib/rpc.ts` | JSON-RPC view calls and final block timestamp. |
| `src/lib/factory.ts` | Factory info and paginated registry reads. |
| `src/lib/rbac.ts` | Contract snapshots, membership discovery, permission helpers. |
| `src/lib/wallet.ts` | Wallet selector setup and signed function calls. |
| `src/lib/roles.ts` | Role id and token id derivation (must match `rbac-common`). |
| `src/lib/favorites.ts` | Network/account-scoped `localStorage` favorites. |
| `src/lib/format.ts` | Token amounts, durations, progress, parsing. |
| `src/lib/accounts.ts` | Account and subaccount validation. |
| `src/lib/async.ts` | Bounded-concurrency `mapLimit`. |

## Limitations

- Every registry scan calls `owner_id()` or `roles_of()` on every registered child. This is
  fine for a hackathon-sized registry but grows linearly; use an indexer for large deployments.
- Reads use a single RPC endpoint with a 20-second timeout. A failed child read is treated as
  "not owned"/"no roles" for that contract rather than failing the whole scan.
- Token metadata is best-effort. If `ft_metadata` fails, amounts are shown in raw units with the
  token account as the symbol.
- Favorites and address/tab state are local to the browser/network/account. Favorites do not
  protect against a malicious contract that later changes its `owner_id`; they only reduce
  noise in the UI.
- The app can verify the factory's stored wasm hash if you set
  `PUBLIC_EXPECTED_CHILD_CODE_HASH`. Without that pin it only displays the hash, so confirm
  `code_hash()` yourself before deploying if that matters.
- The UI is not audited. See the root [Security model](../../README.md#security-model) and
  [Limitations](../../README.md#limitations).
