import { describe, expect, it } from "vitest";

import {
  canManageContract,
  canManageRole,
  findRole,
  isRoleOwner,
  manageableRoles,
} from "../src/lib/rbac";
import type { ContractSnapshot, RoleView } from "../src/lib/types";

function role(overrides: Partial<RoleView> = {}): RoleView {
  return {
    role_id: 1,
    name: "admin",
    owner: "dao.testnet",
    configured_owner: null,
    salary: "0",
    holders: 0,
    ...overrides,
  };
}

function snapshot(overrides: Partial<ContractSnapshot> = {}): ContractSnapshot {
  const base: Omit<ContractSnapshot, "roles"> = {
    accountId: "rbac.dao.testnet",
    ownerId: "dao.testnet",
    salaryToken: "usdc.testnet",
    payoutIntervalNs: "604800000000000",
    token: { decimals: 6, symbol: "USDC", name: "USD Coin", fallback: false },
  };
  return {
    ...base,
    roles: overrides.roles ?? [role()],
    ...overrides,
  };
}

describe("role permissions", () => {
  it("treats the effective role owner as a role owner", () => {
    expect(isRoleOwner(role({ owner: "hr.testnet" }), "hr.testnet")).toBe(true);
    expect(isRoleOwner(role({ owner: "hr.testnet" }), "alice.testnet")).toBe(false);
    expect(isRoleOwner(role(), null)).toBe(false);
  });

  it("allows the contract owner to manage every role", () => {
    const contract = snapshot();
    expect(canManageRole(contract, role({ owner: "hr.testnet" }), "dao.testnet")).toBe(true);
    expect(canManageContract(contract, "dao.testnet")).toBe(true);
  });

  it("allows the effective role owner to manage only that role", () => {
    const contract = snapshot({
      roles: [
        role({ name: "admin", owner: "hr.testnet" }),
        role({ name: "employee", owner: "people.testnet" }),
      ],
    });
    const admin = contract.roles[0];
    const employee = contract.roles[1];

    expect(canManageRole(contract, admin, "hr.testnet")).toBe(true);
    expect(canManageRole(contract, employee, "hr.testnet")).toBe(false);
    expect(manageableRoles(contract, "hr.testnet")).toEqual([admin]);
    expect(manageableRoles(contract, "dao.testnet")).toHaveLength(2);
    expect(manageableRoles(contract, null)).toEqual([]);
  });

  it("finds roles by name", () => {
    const contract = snapshot({
      roles: [role({ name: "admin" }), role({ name: "employee" })],
    });
    expect(findRole(contract, "employee")?.name).toBe("employee");
    expect(findRole(contract, "missing")).toBeUndefined();
  });

  it("requires a connected wallet for management", () => {
    const contract = snapshot();
    expect(canManageRole(contract, contract.roles[0], null)).toBe(false);
    expect(canManageContract(contract, null)).toBe(false);
  });
});
