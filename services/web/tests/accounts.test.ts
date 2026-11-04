import { describe, expect, it } from "vitest";

import {
  childAccountId,
  isValidAccountId,
  isValidSubaccountName,
  subaccountNameError,
} from "../src/lib/accounts";

describe("account ids", () => {
  it("accepts valid NEAR accounts", () => {
    for (const id of ["alice.near", "a-b.testnet", "a_b.testnet", "123.testnet"]) {
      expect(isValidAccountId(id), id).toBe(true);
    }
  });

  it("rejects invalid accounts", () => {
    for (const id of [
      "",
      "a",
      "Alice.near",
      "alice near",
      "-alice.near",
      "alice-.near",
      "alice..near",
      "alice.near-",
      `${"a".repeat(65)}`,
    ]) {
      expect(isValidAccountId(id), id).toBe(false);
    }
  });
});

describe("subaccount names", () => {
  it("validates labels", () => {
    expect(isValidSubaccountName("acme")).toBe(true);
    expect(isValidSubaccountName("acme-2")).toBe(true);
    expect(isValidSubaccountName("acme_2")).toBe(true);
    expect(isValidSubaccountName("Acme")).toBe(false);
    expect(isValidSubaccountName("-acme")).toBe(false);
    expect(isValidSubaccountName("acme-")).toBe(false);
    expect(isValidSubaccountName("ac.me")).toBe(false);
  });

  it("builds child accounts", () => {
    expect(childAccountId("acme", "factory.testnet")).toBe("acme.factory.testnet");
  });

  it("rejects names that make the child too long", () => {
    const factory = "factory.testnet";
    expect(subaccountNameError("acme", factory)).toBeNull();
    expect(subaccountNameError("a".repeat(60), factory)).toContain("longer than 64");
  });

  it("returns actionable errors", () => {
    expect(subaccountNameError("", "factory.testnet")).toContain("Choose a name");
    expect(subaccountNameError("Acme", "factory.testnet")).toContain("lowercase");
  });
});
