# payrole-oidc

An OpenID Connect provider that authenticates NEAR accounts with **NEP-413** and authorizes
them using the role NFTs in a deployed [`payrole-nft`](../../contracts/payrole-nft) contract.

Part of the [PayRole](../../README.md) workspace. The root README covers the contract,
architecture, and security model; this file focuses on running the service.

## How a login works

1. An OIDC client redirects to `/authorize` with PKCE (`S256`), a registered `client_id` and
   exact `redirect_uri`, and optional `state`/`nonce`.
2. The provider renders a login page with a NEP-413 message and a 256-bit nonce.
3. The user signs the message with a NEAR wallet key. The page posts the account id, public
   key, and signature to `/login`.
4. The provider verifies the signature, optionally checks that the public key is an access
   key of the claimed account, then reads `roles_of` from the contract and checks the
   client's `required_roles`.
5. The user is redirected with a one-time authorization code. The client exchanges it at
   `/token` for an RS256 access token and ID token.
6. `/userinfo` and `/introspect` re-read roles from the chain. If a required role has been
   burned, the session is revoked.

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/` | Small landing page listing clients. |
| `GET` | `/healthz` | Liveness check. |
| `GET` | `/.well-known/openid-configuration` | OIDC discovery document. |
| `GET` | `/.well-known/oauth-authorization-server` | Same document for OAuth clients. |
| `GET` | `/jwks.json` | RSA public signing key. |
| `GET` | `/authorize` | Start the authorization-code flow. |
| `POST` | `/login` | Complete the NEP-413 login. |
| `POST` | `/token` | Exchange a code for tokens. |
| `GET`, `POST` | `/userinfo` | Bearer-token user info with live roles. |
| `POST` | `/introspect` | Token introspection with live roles. |
| `POST` | `/logout` | Revoke a session using an access token or `id_token_hint`. |

## Configuration

Copy [`config.example.toml`](config.example.toml) to `config.toml` (git-ignored) and edit it.

### Top-level fields

| Field | Default | Notes |
| --- | --- | --- |
| `issuer` | required | Public HTTPS URL, no trailing slash. Used as `iss` and discovery base. |
| `listen` | `127.0.0.1:8080` | Socket address to bind. |
| `rpc_url` | required | NEAR JSON-RPC used for `roles_of` and `view_access_key`. |
| `contract_id` | required | Deployed `payrole-nft` account. |
| `clients` | required | At least one `[[clients]]` table. |
| `nep413_recipient` | issuer host | Must match the recipient used by the wallet. |
| `nep413_callback_url` | unset | If set, it is included in the signed payload; wallets must use the same value. |
| `access_token_ttl_secs` | `900` | Also the session lifetime. |
| `id_token_ttl_secs` | `900` | ID-token lifetime. |
| `authorization_code_ttl_secs` | `300` | One-time code lifetime. |
| `pending_login_ttl_secs` | `300` | Login challenge lifetime. |
| `signing_key_path` | unset | PKCS#8 PEM RSA private key. Omit to generate an ephemeral key. |
| `max_pending_logins` | `10000` | Maximum number of live pending-login challenges. New logins get HTTP 429 when full. |
| `store_sweep_interval_secs` | `60` | How often a background task prunes expired pending logins, codes and sessions. |
| `verify_access_key` | `true` | Require the NEP-413 key to be an access key of the account. |
| `allowed_scopes` | `["openid","profile","roles"]` | Must contain `openid`. |
| `allowed_origins` | `[]` | Exact browser origins allowed to call discovery, `/token`, and `/userinfo` with CORS. No path, query, or trailing slash. |

### Client fields

| Field | Notes |
| --- | --- |
| `client_id` | Required, unique. |
| `name` | Shown on the login page. |
| `redirect_uris` | Required non-empty list; matched exactly. |
| `client_secret` | Optional. If present, the client is confidential and must authenticate at `/token` (and `/introspect` when it sends credentials). |
| `required_roles` | All of these on-chain roles must be held at login. Rechecked by `/userinfo` and `/introspect`. |
| `allowed_scopes` | Optional per-client restriction; defaults to the global list. |

Public clients (no secret) must use PKCE. Confidential clients can use
`client_secret_basic` or `client_secret_post`.

### Hosted PayRole web client

The static web app in [`services/web`](../web) is a public PKCE client. It needs a client entry
whose redirect URI is the exact callback page, and it needs CORS because discovery, `/token`,
and `/userinfo` are browser `fetch` calls from another origin:

```toml
allowed_origins = ["https://near-payrole.vercel.app"]

[[clients]]
client_id = "payrole-web"
name = "PayRole web"
redirect_uris = ["https://near-payrole.vercel.app/oidc/callback"]
required_roles = []
```

Keep `required_roles` empty so any wallet can sign in and inspect its own access; the app reads
role NFTs from the registry itself and renders ✓ / ✗ for configured access labels. Add a role
only if the login itself should be gated. For a different host, replace the origin and callback
with that deployment's exact values, and set the matching `PUBLIC_OIDC_*` variables when building
the web app.

## Running

```bash
# From the repository root.
cargo run -p payrole-oidc --release -- services/oidc/config.toml

# Or point at a config with an environment variable.
PAYROLE_OIDC_CONFIG=/etc/payrole-oidc/config.toml payrole-oidc
```

Generate a persistent signing key before the first production start:

```bash
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out services/oidc/signing-key.pem
chmod 600 services/oidc/signing-key.pem
```

Set `signing_key_path` in the config. If it is omitted, the service logs a warning and
generates an ephemeral key; every restart invalidates all issued tokens. In production always
use a persistent key file (mounted as a secret) and back it up.

Put the service behind a TLS reverse proxy (Caddy, nginx, a cloud load balancer) whose public
hostname matches `issuer`. Do not terminate TLS with a different public URL: `iss`, discovery
URLs, and the NEP-413 recipient are all derived from `issuer`.

Also add rate limiting at the reverse proxy for `/login`, `/authorize`, `/token` and
`/introspect`. The service caps the number of live pending logins (`max_pending_logins`) and
returns HTTP 429 with `error=temporarily_unavailable` when it is full, but it does not implement
per-IP request throttling itself.

## Docker

Build from the repository root so the workspace and lockfile are available:

```bash
docker build -f services/oidc/Dockerfile -t payrole-oidc .

docker run --rm -p 8080:8080 \
  -v "$PWD/services/oidc/config.toml:/app/config.toml:ro" \
  -v "$PWD/services/oidc/signing-key.pem:/app/signing-key.pem:ro" \
  payrole-oidc
```

The image runs as an unprivileged user and uses `rustls` (no OpenSSL runtime dependency).

## Client example

Generate a verifier and challenge:

```bash
VERIFIER=$(openssl rand -base64 64 | tr -dc 'A-Za-z0-9._~-' | cut -c1-64)
CHALLENGE=$(printf '%s' "$VERIFIER" \
  | openssl dgst -sha256 -binary \
  | openssl base64 -A \
  | tr '+/' '-_' \
  | tr -d '=')
```

Open (URL-encode the parameters in a real client):

```text
https://auth.example.com/authorize?response_type=code
  &client_id=demo-app
  &redirect_uri=https%3A%2F%2Fapp.example.com%2Fcallback
  &scope=openid%20profile%20roles
  &state=random-state
  &nonce=random-nonce
  &code_challenge=<CHALLENGE>
  &code_challenge_method=S256
```

After signing the NEP-413 message and receiving the code:

```bash
curl -sS -X POST https://auth.example.com/token \
  -H 'content-type: application/x-www-form-urlencoded' \
  --data-urlencode grant_type=authorization_code \
  --data-urlencode code="$CODE" \
  --data-urlencode redirect_uri=https://app.example.com/callback \
  --data-urlencode client_id=demo-app \
  --data-urlencode code_verifier="$VERIFIER"
```

Validate the returned JWT against `/jwks.json`, then read live roles:

```bash
curl -sS -H "Authorization: Bearer $ACCESS_TOKEN" https://auth.example.com/userinfo
```

For confidential clients, add HTTP Basic authentication or `client_secret` in the form body.

## Token claims

Access token:

```json
{
  "iss": "https://auth.example.com",
  "sub": "alice.testnet",
  "aud": "demo-app",
  "exp": 1760000000,
  "iat": 1759999100,
  "jti": "<session id>",
  "client_id": "demo-app",
  "scope": "openid profile roles",
  "roles": ["admin", "employee"],
  "token_use": "access"
}
```

ID token: the same identity fields plus `auth_time`, `sid`, optional `nonce`,
`preferred_username`, and `token_use="id"`.

## Operations and security notes

- **Access-key verification is essential.** With `verify_access_key=true` (the default), a
  NEP-413 signature is accepted only if the signing key is a current access key of the
  claimed account. Turning it off lets anyone claim any account id.
- The RPC endpoint is trusted for role and access-key answers. Use a reliable provider.
- The store is in-memory: run one replica, or add a shared persistent store before scaling.
  A background sweeper prunes expired pending logins, authorization codes and sessions every
  `store_sweep_interval_secs`. Pending logins are capped at `max_pending_logins`; when full,
  `/authorize` returns HTTP 429 instead of growing memory without bound.
- There are no refresh tokens. Clients re-run the authorization-code flow when the access
  token expires.
- `/userinfo` and `/introspect` see role changes immediately and revoke the session if a
  required role disappears. A client that only validates JWTs locally will not see
  revocation until expiry.
- `/introspect` allows unauthenticated requests for public-client tokens; tokens issued to
  confidential clients require matching client credentials. Protect the endpoint at the
  network layer if you need stronger isolation.
- Keep the signing key secret and backed up. There is one key; rotation is manual.
- `allowed_origins` is a browser CORS allow-list, not an authentication boundary. Requests
  without an `Origin` header, server-to-server clients, and top-level redirects are unaffected.
  Add only origins you control.
- No per-IP rate limiting, consent UI, dynamic client registration, or audit logging is included.
  Add request rate limiting at the reverse proxy.

## Tests

```bash
cargo test -p payrole-oidc
```

This runs the unit tests and the end-to-end HTTP flow in `tests/oidc_flow.rs` (discovery,
JWKS, PKCE, code binding/reuse/expiry, confidential clients, login signature/access-key
checks, role requirements, live revocation, logout, introspection, CORS, and the pending-login
cap).
