import { describe, expect, it } from "vitest";

import { roleId, tokenId } from "../src/lib/roles";

describe("role ids", () => {
  it("matches the Rust test vectors", async () => {
    expect(await roleId("admin")).toBe("10117748766825055253");
    expect(await roleId("employee")).toBe("3448633025363786332");
    expect(await roleId("manager")).toBe("7990692413185102085");
    expect(await roleId("hr.manager")).toBe("6339347599350065209");
    expect(await roleId("Treasury")).toBe("7701258499861874806");
  });

  it("is case sensitive", async () => {
    expect(await roleId("Admin")).not.toBe(await roleId("admin"));
  });

  it("uses the decimal role id as the token id", async () => {
    expect(await tokenId("admin")).toBe("10117748766825055253");
  });
});
