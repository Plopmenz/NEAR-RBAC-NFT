/**
 * Role ids must match `payrole-common`:
 *
 *   role_id = u64::from_be_bytes(SHA-256(role_name)[0..8])
 *
 * The id is returned as a decimal string because JavaScript numbers lose precision above 2^53.
 */
export async function roleId(roleName: string): Promise<string> {
  const data = new TextEncoder().encode(roleName);
  const digest = await globalThis.crypto.subtle.digest("SHA-256", data);
  return new DataView(digest).getBigUint64(0, false).toString();
}

/**
 * Builds the NEP-245 token id used by the contract.
 *
 * Each role is one token type, so the token id is just the decimal role id. Membership is a
 * balance of 1 on that token type.
 */
export async function tokenId(roleName: string): Promise<string> {
  return roleId(roleName);
}
