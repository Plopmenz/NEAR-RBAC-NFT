const ACCOUNT_ID_RE = /^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/;
const SUBACCOUNT_NAME_RE = /^[a-z0-9](?:[a-z0-9_-]*[a-z0-9])?$/;

/** Returns true for a syntactically valid NEAR account id. */
export function isValidAccountId(value: string): boolean {
  return value.length >= 2 && value.length <= 64 && ACCOUNT_ID_RE.test(value);
}

/** Returns true for a valid single subaccount label (no dots). */
export function isValidSubaccountName(value: string): boolean {
  return value.length > 0 && SUBACCOUNT_NAME_RE.test(value);
}

/** Builds the child account id deployed by the factory. */
export function childAccountId(name: string, factoryContractId: string): string {
  return `${name}.${factoryContractId}`;
}

/** Returns an error message for a subaccount label, or null when it is valid. */
export function subaccountNameError(
  name: string,
  factoryContractId: string,
): string | null {
  if (!name) return "Choose a name for the contract account.";
  if (!isValidSubaccountName(name)) {
    return "Use lowercase letters, digits, '-' or '_', starting and ending with a letter or digit.";
  }
  if (name.length + 1 + factoryContractId.length > 64) {
    return "The resulting account id would be longer than 64 characters.";
  }
  return null;
}
