export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const PREFIX = "rbac-nft:favorites:v1";

export function favoritesKey(network: string, accountId: string): string {
  return `${PREFIX}:${network}:${accountId}`;
}

export function loadFavorites(
  storage: StorageLike,
  network: string,
  accountId: string,
): Set<string> {
  try {
    const raw = storage.getItem(favoritesKey(network, accountId));
    if (!raw) return new Set();
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.filter((value): value is string => typeof value === "string"));
  } catch {
    return new Set();
  }
}

export function saveFavorites(
  storage: StorageLike,
  network: string,
  accountId: string,
  favorites: Iterable<string>,
): void {
  try {
    storage.setItem(
      favoritesKey(network, accountId),
      JSON.stringify([...new Set(favorites)].sort()),
    );
  } catch {
    // Storage can be unavailable (private mode, quota); favoriting is best-effort.
  }
}

export function toggleFavorite(
  storage: StorageLike,
  network: string,
  accountId: string,
  contractId: string,
): Set<string> {
  const favorites = loadFavorites(storage, network, accountId);
  if (favorites.has(contractId)) {
    favorites.delete(contractId);
  } else {
    favorites.add(contractId);
  }
  saveFavorites(storage, network, accountId, favorites);
  return favorites;
}
