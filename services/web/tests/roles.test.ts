import { describe, expect, it } from "vitest";

import { roleId, tokenId } from "../src/lib/roles";

describe("role ids", () => {
  it("matches the Rust test vectors", async () => {
    expect(await roleId("admin")).toBe(2_355_721_957);
    expect(await roleId("employee")).toBe(802_947_447);
    expect(await roleId("manager")).toBe(1_860_478_057);
    expect(await roleId("hr.manager")).toBe(1_475_994_381);
    expect(await roleId("Treasury")).toBe(1_793_088_973);
  });

  it("is case sensitive", async () => {
    expect(await roleId("Admin")).not.toBe(await roleId("admin"));
  });

  it("encodes token ids like the contract", async () => {
    expect(await tokenId("admin", "alice.near")).toBe("2355721957:alice.near");
  });
});
