# PayRole Web

An Astro + TypeScript frontend for deploying and managing [`payrole-nft`](../../contracts/payrole-nft)
contracts through the [`payrole-factory`](../../contracts/payrole-factory).

The app is fully static. It reads NEAR JSON-RPC directly and signs writes with
[`@near-wallet-selector`](https://github.com/near/wallet-selector); there is no backend or
indexer.

## What it does

- **Deploy** a new `payrole-nft` at `{name}.{factory}` in one wallet transaction. The owner can be
  any account, including a DAO or Trezu. If `PUBLIC_EXPECTED_CHILD_CODE_HASH` is set, deployment
  is blocked unless the immutable factory reports exactly that child wasm hash.
- **Owned contracts** scans the factory registry, verifies each child's live `owner_id()`, and
  shows contracts currently owned by the scanned address. Each card shows the contract's
  salary-token balance next to `accrued_liabilities()`, marked funded or underfunded. Contracts
  that use the configured demo token get a free **Fund 100** button that mints demo NEP-141 into
  the contract. Favorites are stored in `localStorage` so spam contracts deployed with your
  account as owner can be hidden.
- **My roles** scans the registry for role memberships held by an address, then shows salary,
  last claim, a payout-duration progress bar, and the currently claimable amount with a claim
  button.
- **OIDC access** signs in through an external OpenID Connect provider using the
  authorization-code flow with PKCE (public client, no secret), shows the authenticated NEAR
  account and live `/userinfo` roles, and renders configured access labels with ✓ / ✗ against
  on-chain role NFTs. The role → label mapping is configured per contract in
  **Contract administration** and stored in this browser.
- Role owners can mint and burn memberships from either tab. Minting attaches the contract's
  `required_mint_deposit()`; burning pays accrued salary first and asks for confirmation.
  Contract owners get controls for role creation, role salaries, role-owner delegation, salary
  token/interval, ownership transfer, and OIDC access labels.

Discovery is registry-based instead of indexed. The registry is treated as a hint: the app
always calls `owner_id()` / `roles_of()` on the child contract before trusting it.

## Prerequisites

- Node.js **22.23.4+** (Astro 7 requirement) and npm 9.6.5+
- A deployed [`payrole-factory`](../../contracts/payrole-factory) initialized once with the `payrole-nft`
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
| `PUBLIC_FACTORY_CONTRACT_ID` | yes | Factory account, e.g. `payrole-factory.testnet`. Without it, deployment and discovery are disabled and a banner is shown. |
| `PUBLIC_EXPECTED_CHILD_CODE_HASH` | no | SHA-256 of the reviewed `payrole-nft` wasm. When set, the app refuses to deploy if `code_hash()` differs. |
| `PUBLIC_EXPLORER_URL` | no | Explorer base URL. Defaults to NEARBlocks for the network. |
| `PUBLIC_SITE_URL` | no | Absolute public URL used for the canonical link and Open Graph/Twitter image tags, and as the default OIDC redirect origin. When unset those tags are omitted and the OIDC callback uses the current browser origin. |
| `PUBLIC_SALARY_TOKEN_ID` | no | Demo NEP-141 created by `scripts/deploy-salary-token.sh`. When set, the deploy form pre-fills it and owned contracts using it get a free **Fund** button. Leave empty to hide demo funding. |
| `PUBLIC_SALARY_TOKEN_DECIMALS` | no | Decimals used to format the demo token. Defaults to `6`. |
| `PUBLIC_SALARY_TOKEN_SYMBOL` | no | Display symbol for the demo token. Defaults to `MOCK`. |
| `PUBLIC_DEMO_FUND_AMOUNT` | no | Human-readable amount minted by the free funding button. Defaults to `100`. |
| `PUBLIC_OIDC_ISSUER` | no | OIDC issuer base URL, e.g. `https://auth.example.com`. Empty disables the access tab's sign-in button. |
| `PUBLIC_OIDC_CLIENT_ID` | no | Registered public client id, e.g. `payrole-web`. |
| `PUBLIC_OIDC_SCOPES` | no | Requested scopes. Defaults to `openid profile roles`. |
| `PUBLIC_OIDC_REDIRECT_URI` | no | Registered redirect URI. Defaults to `${PUBLIC_SITE_URL}/oidc/callback`, or `${window.location.origin}/oidc/callback` when no site URL is set. |

These are **build-time** variables. Rebuild after changing them. A missing factory is not
defaulted on purpose: silently pointing at the wrong registry would be worse than disabling the
app.

The OIDC provider must register the exact redirect URI and allow this site's origin in its
`allowed_origins` list so browser `fetch` calls to discovery, `/token`, and `/userinfo` work.
See [`services/oidc`](../oidc) for the provider side.

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
Pages, Netlify, Vercel, S3, …) with the environment variables set at build time.

The app needs no server, but the RPC endpoint is trusted for read results. Use a reliable
provider for production.

The hosted demo at <https://near-payrole.vercel.app> is built with at least:

```bash
PUBLIC_NEAR_NETWORK=testnet
PUBLIC_FACTORY_CONTRACT_ID=payrole-factory.testnet
PUBLIC_SITE_URL=https://near-payrole.vercel.app
PUBLIC_SALARY_TOKEN_ID=<token printed by scripts/deploy-salary-token.sh>
PUBLIC_OIDC_ISSUER=<public OIDC issuer URL>
PUBLIC_OIDC_CLIENT_ID=payrole-web
```

Set `PUBLIC_EXPECTED_CHILD_CODE_HASH` to the `sha256sum res/payrole_nft.wasm` value from the
reviewed build if you want the deploy form to hard-fail when the factory stores different code.

## Usage

1. Connect a NEAR wallet. The connected account pays for deployments and signs admin calls.
2. In **Deploy**, choose a subaccount name, the contract owner (your DAO/Trezu), the NEP-141
   salary token, and the payout interval (30 days by default). The factory reports the minimum
   deposit; the app pre-fills it. The panel shows the factory's child wasm hash and marks it
   pinned when it matches `PUBLIC_EXPECTED_CHILD_CODE_HASH`. Excess deposit above the factory's
   fixed registry reserve is forwarded to the child.
3. In **Owned contracts**, scan the owner address. Favorite the contracts you care about. If a
   contract uses `PUBLIC_SALARY_TOKEN_ID`, press **Fund** to mint the configured demo amount into
   it.
4. Open a contract's **Contract administration** panel to create roles, set salaries, delegate
   role owners, map roles to OIDC access labels, change payroll configuration, or transfer
   ownership.
5. In **My roles**, scan a member address. Claim salary when the claimable amount is non-zero.
6. In **OIDC access**, press **Sign in with OIDC**, approve the NEP-413 message, and review the
   ✓ / ✗ access labels. **Refresh roles** re-reads on-chain memberships without signing in again.

Minting an existing role or burning a missing role fails on-chain. Minting attaches the
contract-reported storage deposit. Burning requests `pay_pending: true`, so accrued salary is
transferred first; the confirmation explains this and a failed transfer keeps the membership.
Write actions show a pending/disabled button and report success or error in the status line.
The page ships a key/role-badge mark, a hero flow diagram, and a social preview image
(`public/og.png`) for links shared from a `PUBLIC_SITE_URL`.

OIDC access labels are a presentation layer, not a contract setting. They are stored in this
browser under `payrole-nft:access:v1:<network>` and aggregated across roles for each contract;
holding any role that grants a label is enough to show ✓. The role check itself always reads
live role NFTs. If no labels are configured, role names are used as sample labels.

## Module map

| File | Purpose |
| --- | --- |
| `src/pages/index.astro` | Page markup, branding, hero diagram, social tags. |
| `src/pages/oidc/callback.astro` | OIDC redirect callback: validates state and exchanges the code. |
| `src/scripts/app.ts` | Entry point: hydrates local state, restores the URL, binds events and starts wallet loading. |
| `src/scripts/app/state.ts` | Shared render state, runtime counters/caches, interval units. |
| `src/scripts/app/dom.ts` | Element lookup, escaping, status/notice helpers, busy buttons. |
| `src/scripts/app/views.ts` | Contract cards, role/member tables, admin forms, rendering. |
| `src/scripts/app/intervals.ts` | Payout interval formatting and parsing. |
| `src/scripts/app/actions.ts` | Registry scans, wallet actions, deploy/mint/burn/claim/admin calls, funding, OIDC and access scans. |
| `src/scripts/app/events.ts` | DOM event wiring and per-form busy state. |
| `src/lib/rpc.ts` | JSON-RPC view calls and final block timestamp. |
| `src/lib/factory.ts` | Factory info and paginated registry reads. |
| `src/lib/payrole.ts` | Contract snapshots, membership discovery, permission helpers. |
| `src/lib/access.ts` | Browser-local role → access-label mapping and ✓ / ✗ calculation. |
| `src/lib/oidc.ts` | OIDC discovery, PKCE, callback token exchange, session storage, userinfo. |
| `src/lib/wallet.ts` | Wallet selector setup and signed function calls. |
| `src/lib/roles.ts` | Role id and token id derivation (must match `payrole-common`). |
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
- OIDC access labels are local to the browser/network as well. They are not shared between
  devices and do not grant anything by themselves; the page always re-checks live role NFTs.
- OIDC sign-in requires a reachable issuer that registers this deployment's exact callback URI
  and allows this origin in `allowed_origins`. Without it, the access tab falls back to wallet
  preview and shows a configuration warning.
- The app can verify the factory's stored wasm hash if you set
  `PUBLIC_EXPECTED_CHILD_CODE_HASH`. Without that pin it only displays the hash, so confirm
  `code_hash()` yourself before deploying if that matters.
- The UI is not audited. See the root [Security model](../../README.md#security-model) and
  [Limitations](../../README.md#limitations).
