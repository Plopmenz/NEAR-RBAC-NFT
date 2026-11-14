import { describe, expect, it } from "vitest";

import {
  beginOidcLogin,
  buildAuthorizeUrl,
  clearOidcSession,
  completeOidcLogin,
  pkceChallenge,
  randomUrlSafe,
  readOidcSession,
} from "../src/lib/oidc";
import type { AppConfig } from "../src/lib/types";

class MemoryStorage implements Storage {
  private values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  clear(): void {
    this.values.clear();
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

function config(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    network: "testnet",
    rpcUrl: "https://rpc.testnet.near.org",
    factoryContractId: "payrole-factory.testnet",
    expectedChildCodeHash: "",
    explorerUrl: "https://testnet.nearblocks.io",
    siteUrl: "https://near-payrole.vercel.app",
    salaryTokenId: "mock-ft.dao.testnet",
    salaryTokenDecimals: 6,
    salaryTokenSymbol: "MOCK",
    demoFundAmount: "100",
    oidcIssuer: "https://auth.example.com",
    oidcClientId: "payrole-web",
    oidcScopes: "openid profile roles",
    oidcRedirectUri: "https://near-payrole.vercel.app/oidc/callback",
    ...overrides,
  };
}

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

describe("PKCE helpers", () => {
  it("matches the RFC 7636 S256 vector", async () => {
    await expect(
      pkceChallenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
    ).resolves.toBe("E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
  });

  it("creates URL-safe random values", () => {
    const value = randomUrlSafe(32);
    expect(value).toHaveLength(43);
    expect(value).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it("builds a complete authorization URL", () => {
    const url = new URL(
      buildAuthorizeUrl("https://auth.example.com/authorize", {
        clientId: "payrole-web",
        redirectUri: "https://near-payrole.vercel.app/oidc/callback",
        scope: "openid profile roles",
        state: "state",
        nonce: "nonce",
        challenge: "challenge",
      }),
    );
    expect(url.searchParams.get("response_type")).toBe("code");
    expect(url.searchParams.get("client_id")).toBe("payrole-web");
    expect(url.searchParams.get("code_challenge_method")).toBe("S256");
    expect(url.searchParams.get("code_challenge")).toBe("challenge");
    expect(url.searchParams.get("scope")).toBe("openid profile roles");
  });
});

describe("OIDC authorization-code flow", () => {
  it("stores PKCE state and completes the token exchange", async () => {
    const storage = new MemoryStorage();
    const fetcher = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/.well-known/openid-configuration")) {
        return jsonResponse({
          issuer: "https://auth.example.com",
          authorization_endpoint: "https://auth.example.com/authorize",
          token_endpoint: "https://auth.example.com/token",
          userinfo_endpoint: "https://auth.example.com/userinfo",
        });
      }
      if (url.endsWith("/token")) {
        return jsonResponse({
          access_token: "access-token",
          id_token: "id-token",
          token_type: "Bearer",
          expires_in: 900,
          scope: "openid profile roles",
        });
      }
      if (url.endsWith("/userinfo")) {
        expect(init?.headers).toMatchObject({ authorization: "Bearer access-token" });
        return jsonResponse({ sub: "alice.testnet", roles: ["admin"] });
      }
      return jsonResponse({ error: "unexpected" }, 500);
    }) as typeof fetch;

    let authorizeUrl = "";
    await beginOidcLogin(config(), {
      storage,
      fetcher,
      navigate: (url) => {
        authorizeUrl = url;
      },
    });
    const authorize = new URL(authorizeUrl);
    const state = authorize.searchParams.get("state");
    const challenge = authorize.searchParams.get("code_challenge");
    expect(state).toBeTruthy();
    expect(challenge).toBeTruthy();

    const session = await completeOidcLogin(config(), {
      href: `https://near-payrole.vercel.app/oidc/callback?code=code-1&state=${state}`,
      storage,
      fetcher,
    });
    expect(session.accountId).toBe("alice.testnet");
    expect(session.roles).toEqual(["admin"]);
    expect(session.expiresAt).toBeGreaterThan(Date.now());
    expect(readOidcSession(storage)?.accountId).toBe("alice.testnet");
  });

  it("rejects a callback whose state does not match", async () => {
    const storage = new MemoryStorage();
    const fetcher = (async () =>
      jsonResponse({
        issuer: "https://auth.example.com",
        authorization_endpoint: "https://auth.example.com/authorize",
        token_endpoint: "https://auth.example.com/token",
        userinfo_endpoint: "https://auth.example.com/userinfo",
      })) as typeof fetch;

    await beginOidcLogin(config(), {
      storage,
      fetcher,
      navigate: () => {},
    });

    await expect(
      completeOidcLogin(config(), {
        href: "https://near-payrole.vercel.app/oidc/callback?code=code-1&state=wrong",
        storage,
        fetcher,
      }),
    ).rejects.toThrow(/state/i);
  });

  it("surfaces provider errors from the callback", async () => {
    await expect(
      completeOidcLogin(config(), {
        href: "https://near-payrole.vercel.app/oidc/callback?error=access_denied&error_description=missing%20role",
        storage: new MemoryStorage(),
        fetcher: (async () => jsonResponse({})) as typeof fetch,
      }),
    ).rejects.toThrow("missing role");
  });

  it("removes the session and pending login when signing out", () => {
    const storage = new MemoryStorage();
    storage.setItem(
      "payrole-nft:oidc:session:v1",
      JSON.stringify({
        issuer: "https://auth.example.com",
        clientId: "payrole-web",
        accessToken: "access-token",
        idToken: "",
        userinfoEndpoint: "https://auth.example.com/userinfo",
        expiresAt: Date.now() + 60_000,
        scope: "openid",
        accountId: "alice.testnet",
        roles: ["admin"],
      }),
    );
    storage.setItem("payrole-nft:oidc:pending:v1", "{}");

    clearOidcSession(storage);

    expect(readOidcSession(storage)).toBeNull();
    expect(storage.getItem("payrole-nft:oidc:pending:v1")).toBeNull();
  });

  it("does not restore an expired session after a refresh", () => {
    const storage = new MemoryStorage();
    storage.setItem(
      "payrole-nft:oidc:session:v1",
      JSON.stringify({
        issuer: "https://auth.example.com",
        clientId: "payrole-web",
        accessToken: "access-token",
        idToken: "",
        userinfoEndpoint: "https://auth.example.com/userinfo",
        expiresAt: Date.now() - 1_000,
        scope: "openid",
        accountId: "alice.testnet",
        roles: ["admin"],
      }),
    );

    expect(readOidcSession(storage)).toBeNull();
    expect(storage.getItem("payrole-nft:oidc:session:v1")).toBeNull();
  });
});
