import { describe, expect, it } from "vitest";

import {
  accessItems,
  labelsForRole,
  loadAccessMap,
  normalizeLabels,
  parseAccessMap,
  setRoleLabels,
} from "../src/lib/access";

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

describe("access labels", () => {
  it("normalizes comma-separated labels", () => {
    expect(normalizeLabels(" admin , billing:read, admin, , ")).toEqual([
      "admin",
      "billing:read",
    ]);
  });

  it("ignores malformed local storage values", () => {
    expect(parseAccessMap(null)).toEqual({});
    expect(parseAccessMap("not json")).toEqual({});
    expect(parseAccessMap("[]")).toEqual({});
    expect(parseAccessMap('{"contract":{"admin":"nope"}}')).toEqual({});
    expect(
      parseAccessMap('{"contract":{"admin":["admin-panel"],"employee":[]}}'),
    ).toEqual({ contract: { admin: ["admin-panel"] } });
  });

  it("persists and clears labels per contract and role", () => {
    const storage = new MemoryStorage();
    setRoleLabels(storage, "testnet", "payrole.dao.testnet", "admin", [
      "admin-panel",
      "billing:read",
    ]);
    setRoleLabels(storage, "testnet", "payrole.dao.testnet", "employee", ["dashboard"]);

    let map = loadAccessMap(storage, "testnet");
    expect(labelsForRole(map, "payrole.dao.testnet", "admin")).toEqual([
      "admin-panel",
      "billing:read",
    ]);

    map = setRoleLabels(storage, "testnet", "payrole.dao.testnet", "admin", []);
    expect(labelsForRole(map, "payrole.dao.testnet", "admin")).toEqual([]);
    expect(labelsForRole(map, "payrole.dao.testnet", "employee")).toEqual(["dashboard"]);
  });

  it("aggregates labels across roles and checks held roles", () => {
    const map = {
      contract: {
        admin: ["admin-panel", "billing:read"],
        employee: ["dashboard", "billing:read"],
      },
    };

    expect(
      accessItems(map, "contract", ["admin", "employee", "viewer"], ["employee"]),
    ).toEqual([
      { label: "admin-panel", granted: false, roles: ["admin"] },
      { label: "billing:read", granted: true, roles: ["admin", "employee"] },
      { label: "dashboard", granted: true, roles: ["employee"] },
    ]);
  });

  it("falls back to role names when no labels are configured", () => {
    expect(accessItems({}, "contract", ["viewer", "admin"], ["admin"])).toEqual([
      { label: "admin", granted: true, roles: [] },
      { label: "viewer", granted: false, roles: [] },
    ]);
  });
});
