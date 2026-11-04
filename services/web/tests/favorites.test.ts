import { describe, expect, it } from "vitest";

import {
  favoritesKey,
  loadFavorites,
  saveFavorites,
  toggleFavorite,
  type StorageLike,
} from "../src/lib/favorites";

class MemoryStorage implements StorageLike {
  private values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

describe("favorites", () => {
  it("namespaces favorites by network and account", () => {
    expect(favoritesKey("testnet", "alice.near")).toContain("testnet");
    expect(favoritesKey("testnet", "alice.near")).not.toBe(
      favoritesKey("mainnet", "alice.near"),
    );
  });

  it("starts empty and ignores malformed data", () => {
    const storage = new MemoryStorage();
    expect(loadFavorites(storage, "testnet", "alice.near").size).toBe(0);
    storage.setItem(favoritesKey("testnet", "alice.near"), "{not json");
    expect(loadFavorites(storage, "testnet", "alice.near").size).toBe(0);
    storage.setItem(favoritesKey("testnet", "alice.near"), '{"a":1}');
    expect(loadFavorites(storage, "testnet", "alice.near").size).toBe(0);
  });

  it("toggles and persists favorites", () => {
    const storage = new MemoryStorage();
    expect(toggleFavorite(storage, "testnet", "alice.near", "b.near").has("b.near")).toBe(true);
    expect(toggleFavorite(storage, "testnet", "alice.near", "a.near").has("a.near")).toBe(true);
    expect([...loadFavorites(storage, "testnet", "alice.near")]).toEqual(["a.near", "b.near"]);
    expect(toggleFavorite(storage, "testnet", "alice.near", "a.near").has("a.near")).toBe(false);
  });

  it("does not leak favorites between accounts", () => {
    const storage = new MemoryStorage();
    saveFavorites(storage, "testnet", "alice.near", ["a.near"]);
    expect(loadFavorites(storage, "testnet", "bob.near").size).toBe(0);
  });
});
