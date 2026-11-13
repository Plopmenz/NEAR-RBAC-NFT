import type { AppConfig, OidcSession } from "./types";

/**
 * Small browser-side OpenID Connect client using the authorization-code flow with PKCE.
 *
 * The app is a public client: it has no secret and never sees a signing key. The ID token is
 * decoded only for display; account identity and live roles come from `/userinfo` over TLS.
 */

const SESSION_KEY = "payrole-nft:oidc:session:v1";
const PENDING_KEY = "payrole-nft:oidc:pending:v1";

export interface OidcDiscovery {
  issuer: string;
  authorization_endpoint: string;
  token_endpoint: string;
  userinfo_endpoint: string;
}

interface PendingLogin {
  issuer: string;
  clientId: string;
  state: string;
  nonce: string;
  verifier: string;
  redirectUri: string;
  tokenEndpoint: string;
  userinfoEndpoint: string;
  createdAt: number;
}

interface TokenResponse {
  access_token?: string;
  id_token?: string;
  token_type?: string;
  expires_in?: number;
  scope?: string;
  error?: string;
  error_description?: string;
}

interface UserInfoResponse {
  sub?: string;
  preferred_username?: string;
  roles?: unknown;
}

export function oidcConfigured(config: AppConfig): boolean {
  return Boolean(config.oidcIssuer && config.oidcClientId);
}

/** Resolves the redirect URI registered with the provider. */
export function redirectUri(config: AppConfig, origin?: string): string {
  if (config.oidcRedirectUri) return config.oidcRedirectUri;
  const base = origin ?? (typeof window === "undefined" ? "" : window.location.origin);
  return base ? `${base}/oidc/callback` : "/oidc/callback";
}

function base64UrlEncode(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

/** Creates a URL-safe random string from the platform CSPRNG. */
export function randomUrlSafe(byteLength = 32): string {
  const bytes = new Uint8Array(byteLength);
  crypto.getRandomValues(bytes);
  return base64UrlEncode(bytes);
}

/** Computes the S256 PKCE challenge for a verifier. */
export async function pkceChallenge(verifier: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return base64UrlEncode(new Uint8Array(digest));
}

/** Builds an authorization request. Kept separate so the URL shape is testable. */
export function buildAuthorizeUrl(
  endpoint: string,
  params: {
    clientId: string;
    redirectUri: string;
    scope: string;
    state: string;
    nonce: string;
    challenge: string;
  },
): string {
  const url = new URL(endpoint);
  url.searchParams.set("response_type", "code");
  url.searchParams.set("client_id", params.clientId);
  url.searchParams.set("redirect_uri", params.redirectUri);
  url.searchParams.set("scope", params.scope);
  url.searchParams.set("state", params.state);
  url.searchParams.set("nonce", params.nonce);
  url.searchParams.set("code_challenge", params.challenge);
  url.searchParams.set("code_challenge_method", "S256");
  return url.toString();
}

export async function fetchDiscovery(
  issuer: string,
  fetcher: typeof fetch = fetch,
): Promise<OidcDiscovery> {
  const base = issuer.replace(/\/+$/, "");
  const response = await fetcher(`${base}/.well-known/openid-configuration`, {
    headers: { accept: "application/json" },
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) {
    throw new Error(`OIDC discovery failed with HTTP ${response.status}.`);
  }
  const payload = (await response.json()) as Partial<OidcDiscovery>;
  if (
    typeof payload.authorization_endpoint !== "string" ||
    typeof payload.token_endpoint !== "string" ||
    typeof payload.userinfo_endpoint !== "string"
  ) {
    throw new Error("OIDC discovery response is missing an endpoint.");
  }
  return {
    issuer: typeof payload.issuer === "string" ? payload.issuer : base,
    authorization_endpoint: payload.authorization_endpoint,
    token_endpoint: payload.token_endpoint,
    userinfo_endpoint: payload.userinfo_endpoint,
  };
}

/** Starts the redirect to the provider. The pending transaction is stored locally by `state`. */
export async function beginOidcLogin(
  config: AppConfig,
  options: {
    storage?: Storage;
    fetcher?: typeof fetch;
    navigate?: (url: string) => void;
  } = {},
): Promise<void> {
  if (!oidcConfigured(config)) {
    throw new Error("OIDC is not configured for this deployment.");
  }
  const storage = options.storage ?? localStorage;
  const fetcher = options.fetcher ?? fetch;
  const discovery = await fetchDiscovery(config.oidcIssuer, fetcher);

  const state = randomUrlSafe(24);
  const nonce = randomUrlSafe(24);
  const verifier = randomUrlSafe(48);
  const challenge = await pkceChallenge(verifier);
  const pending: PendingLogin = {
    issuer: config.oidcIssuer,
    clientId: config.oidcClientId,
    state,
    nonce,
    verifier,
    redirectUri: redirectUri(config),
    tokenEndpoint: discovery.token_endpoint,
    userinfoEndpoint: discovery.userinfo_endpoint,
    createdAt: Date.now(),
  };
  storage.setItem(PENDING_KEY, JSON.stringify(pending));

  const url = buildAuthorizeUrl(discovery.authorization_endpoint, {
    clientId: config.oidcClientId,
    redirectUri: pending.redirectUri,
    scope: config.oidcScopes,
    state,
    nonce,
    challenge,
  });
  (options.navigate ?? ((target) => window.location.assign(target)))(url);
}

function parsePending(raw: string | null): PendingLogin | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Partial<PendingLogin>;
    if (
      typeof value.state === "string" &&
      typeof value.verifier === "string" &&
      typeof value.redirectUri === "string" &&
      typeof value.tokenEndpoint === "string" &&
      typeof value.userinfoEndpoint === "string" &&
      typeof value.clientId === "string"
    ) {
      return value as PendingLogin;
    }
  } catch {
    // Fall through to null.
  }
  return null;
}

function decodeJwtPayload(token: string): Record<string, unknown> {
  const part = token.split(".")[1];
  if (!part) return {};
  const padded = part.replaceAll("-", "+").replaceAll("_", "/").padEnd(
    part.length + ((4 - (part.length % 4)) % 4),
    "=",
  );
  try {
    return JSON.parse(atob(padded)) as Record<string, unknown>;
  } catch {
    return {};
  }
}

function stringRoles(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((role): role is string => typeof role === "string")
    : [];
}

async function fetchUserInfo(
  endpoint: string,
  accessToken: string,
  fetcher: typeof fetch,
): Promise<{ accountId: string; roles: string[] }> {
  const response = await fetcher(endpoint, {
    headers: {
      accept: "application/json",
      authorization: `Bearer ${accessToken}`,
    },
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) {
    throw new Error(`OIDC userinfo failed with HTTP ${response.status}.`);
  }
  const info = (await response.json()) as UserInfoResponse;
  if (typeof info.sub !== "string" || !info.sub) {
    throw new Error("OIDC userinfo response is missing sub.");
  }
  return { accountId: info.sub, roles: stringRoles(info.roles) };
}

export function readOidcSession(storage: Storage = localStorage): OidcSession | null {
  const raw = storage.getItem(SESSION_KEY);
  if (!raw) return null;
  try {
    const session = JSON.parse(raw) as Partial<OidcSession>;
    if (
      typeof session.accessToken !== "string" ||
      typeof session.accountId !== "string" ||
      typeof session.expiresAt !== "number" ||
      session.expiresAt <= Date.now() + 5_000
    ) {
      storage.removeItem(SESSION_KEY);
      return null;
    }
    return {
      issuer: typeof session.issuer === "string" ? session.issuer : "",
      clientId: typeof session.clientId === "string" ? session.clientId : "",
      accessToken: session.accessToken,
      idToken: typeof session.idToken === "string" ? session.idToken : "",
      userinfoEndpoint:
        typeof session.userinfoEndpoint === "string" ? session.userinfoEndpoint : "",
      expiresAt: session.expiresAt,
      scope: typeof session.scope === "string" ? session.scope : "",
      accountId: session.accountId,
      roles: stringRoles(session.roles),
    };
  } catch {
    storage.removeItem(SESSION_KEY);
    return null;
  }
}

export function saveOidcSession(session: OidcSession, storage: Storage = localStorage): void {
  storage.setItem(SESSION_KEY, JSON.stringify(session));
}

export function clearOidcSession(storage: Storage = localStorage): void {
  storage.removeItem(SESSION_KEY);
  storage.removeItem(PENDING_KEY);
}

/** Completes the callback: validates `state`, exchanges the code, and loads live user info. */
export async function completeOidcLogin(
  config: AppConfig,
  options: {
    href?: string;
    storage?: Storage;
    fetcher?: typeof fetch;
  } = {},
): Promise<OidcSession> {
  const storage = options.storage ?? localStorage;
  const fetcher = options.fetcher ?? fetch;
  const href = options.href ?? window.location.href;
  const url = new URL(href);

  const providerError = url.searchParams.get("error");
  if (providerError) {
    throw new Error(
      url.searchParams.get("error_description") || `OIDC login failed: ${providerError}`,
    );
  }

  const code = url.searchParams.get("code");
  const state = url.searchParams.get("state");
  if (!code || !state) {
    throw new Error("OIDC callback is missing the authorization code or state.");
  }

  const pending = parsePending(storage.getItem(PENDING_KEY));
  if (!pending || pending.state !== state) {
    throw new Error("OIDC state does not match this browser session. Try signing in again.");
  }
  if (pending.issuer !== config.oidcIssuer || pending.clientId !== config.oidcClientId) {
    throw new Error("OIDC callback belongs to a different issuer or client.");
  }

  const body = new URLSearchParams({
    grant_type: "authorization_code",
    code,
    redirect_uri: pending.redirectUri,
    client_id: pending.clientId,
    code_verifier: pending.verifier,
  });
  const response = await fetcher(pending.tokenEndpoint, {
    method: "POST",
    headers: {
      accept: "application/json",
      "content-type": "application/x-www-form-urlencoded",
    },
    body,
    signal: AbortSignal.timeout(20_000),
  });
  const payload = (await response.json().catch(() => ({}))) as TokenResponse;
  if (!response.ok || !payload.access_token) {
    throw new Error(
      payload.error_description || payload.error || `OIDC token exchange failed with HTTP ${response.status}.`,
    );
  }

  const idToken = payload.id_token ?? "";
  const claims = decodeJwtPayload(idToken);
  let accountId = typeof claims.sub === "string" ? claims.sub : "";
  let roles = stringRoles(claims.roles);
  try {
    const info = await fetchUserInfo(pending.userinfoEndpoint, payload.access_token, fetcher);
    accountId = info.accountId;
    roles = info.roles;
  } catch {
    // The ID token still identifies the account for display; live roles are best-effort here.
  }
  if (!accountId) {
    throw new Error("OIDC tokens did not identify a NEAR account.");
  }

  const expiresIn = Number(payload.expires_in);
  const session: OidcSession = {
    issuer: pending.issuer,
    clientId: pending.clientId,
    accessToken: payload.access_token,
    idToken,
    userinfoEndpoint: pending.userinfoEndpoint,
    expiresAt: Date.now() + (Number.isFinite(expiresIn) && expiresIn > 0 ? expiresIn : 900) * 1000,
    scope: payload.scope ?? config.oidcScopes,
    accountId,
    roles,
  };
  saveOidcSession(session, storage);
  storage.removeItem(PENDING_KEY);
  return session;
}

/** Re-reads live roles for an existing session. */
export async function refreshOidcSession(
  session: OidcSession,
  options: { storage?: Storage; fetcher?: typeof fetch } = {},
): Promise<OidcSession> {
  const storage = options.storage ?? localStorage;
  const fetcher = options.fetcher ?? fetch;
  if (!session.userinfoEndpoint) return session;
  const info = await fetchUserInfo(session.userinfoEndpoint, session.accessToken, fetcher);
  const updated: OidcSession = { ...session, accountId: info.accountId, roles: info.roles };
  saveOidcSession(updated, storage);
  return updated;
}
