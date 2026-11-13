import { childAccountId, isValidAccountId, subaccountNameError } from "../../lib/accounts";
import { loadAccessMap, normalizeLabels, setRoleLabels } from "../../lib/access";
import { CLAIM_GAS, DEPLOY_GAS, WRITE_GAS } from "../../lib/config";
import {
  listFactoryContracts,
  loadFactoryInfo,
  transactionExplorerUrl,
} from "../../lib/factory";
import { loadFavorites, saveFavorites, toggleFavorite } from "../../lib/favorites";
import { formatNear, parseNear, parseUnits } from "../../lib/format";
import {
  beginOidcLogin,
  clearOidcSession,
  oidcConfigured,
  readOidcSession,
  refreshOidcSession,
} from "../../lib/oidc";
import { loadMembershipContracts, loadOwnedContracts } from "../../lib/payrole";
import { getFinalBlockTimestampNs } from "../../lib/rpc";
import type { FactoryContractRecord } from "../../lib/types";
import {
  initWallet,
  restoreWallet,
  signAndSendFunctionCall,
  transactionHash,
  walletErrorMessage,
} from "../../lib/wallet";
import { byId, setStatus, setStatusLink, showNotice, str } from "./dom";
import { parseInterval } from "./intervals";
import { runtime, state, type Tab } from "./state";
import {
  activeStatus,
  findSnapshot,
  renderAccess,
  renderFactory,
  renderOwned,
  renderRoles,
  renderTabs,
  renderWallet,
} from "./views";

export function updateUrl(): void {
  const url = new URL(window.location.href);
  url.searchParams.set("tab", state.tab);
  if (state.tab === "contracts" && state.ownedAddress) {
    url.searchParams.set("account", state.ownedAddress);
  } else if (state.tab === "roles" && state.rolesAddress) {
    url.searchParams.set("account", state.rolesAddress);
  } else {
    url.searchParams.delete("account");
  }
  window.history.replaceState({}, "", url);
}

/** Loads browser-local access labels and the OIDC session after the app starts. */
export function hydrateLocalState(): void {
  state.accessMap = loadAccessMap(localStorage, state.config.network);
  state.oidcSession = readOidcSession();
}

export function restoreFromUrl(): void {
  const params = new URLSearchParams(window.location.search);
  const tab = params.get("tab");
  if (tab === "deploy" || tab === "contracts" || tab === "roles" || tab === "access") {
    state.tab = tab;
  }
  const account = params.get("account")?.trim() ?? "";
  if (account) {
    state.ownedAddress = account;
    state.rolesAddress = account;
    byId<HTMLInputElement>("owned-address").value = account;
    byId<HTMLInputElement>("roles-address").value = account;
  }
  if (params.get("favorites") === "1") {
    state.favoritesOnly = true;
    byId<HTMLInputElement>("favorites-only").checked = true;
  }
}

async function getRegistry(force = false): Promise<FactoryContractRecord[]> {
  if (!state.config.factoryContractId) {
    throw new Error("Factory contract is not configured.");
  }
  if (!force && runtime.registryCache && Date.now() - runtime.registryCache.fetchedAt < 60_000) {
    return runtime.registryCache.records;
  }
  const records = await listFactoryContracts(
    state.config.rpcUrl,
    state.config.factoryContractId,
  );
  runtime.registryCache = { fetchedAt: Date.now(), records };
  return records;
}

async function latestBlockTimestamp(): Promise<bigint> {
  try {
    return await getFinalBlockTimestampNs(state.config.rpcUrl);
  } catch {
    return BigInt(Date.now()) * 1_000_000n;
  }
}

export async function refreshFactory(): Promise<void> {
  if (!state.config.factoryContractId) {
    renderFactory();
    return;
  }
  try {
    state.factoryInfo = await loadFactoryInfo(
      state.config.rpcUrl,
      state.config.factoryContractId,
    );
    state.factoryError = "";
  } catch (error) {
    state.factoryInfo = null;
    state.factoryError = walletErrorMessage(error);
  }
  renderFactory();
}

export async function scanOwned(force = false): Promise<void> {
  const address = state.ownedAddress.trim();
  const status = byId<HTMLElement>("owned-status");
  if (!isValidAccountId(address)) {
    setStatus(status, "Enter a valid NEAR account id.", "error");
    return;
  }

  state.ownedAddress = address;
  const run = ++runtime.ownedRun;
  state.ownedLoading = true;
  state.ownedScanned = false;
  state.owned = [];
  state.favorites = loadFavorites(localStorage, state.config.network, address);
  renderOwned();
  updateUrl();
  setStatus(status, `Scanning the factory registry for ${address}…`);

  try {
    const [records, blockTimestamp] = await Promise.all([
      getRegistry(force),
      latestBlockTimestamp(),
    ]);
    const owned = await loadOwnedContracts(state.config.rpcUrl, records, address);
    if (run !== runtime.ownedRun) return;
    state.blockTimestampNs = blockTimestamp;
    state.owned = owned;
    state.ownedScanned = true;
    renderOwned();
    setStatus(
      status,
      records.length === 0
        ? "The factory registry is empty. Deploy the first contract in the first tab."
        : `Found ${owned.length} contract(s) currently owned by ${address}.`,
      "ok",
    );
  } catch (error) {
    if (run !== runtime.ownedRun) return;
    state.owned = [];
    state.ownedScanned = true;
    renderOwned();
    setStatus(status, walletErrorMessage(error), "error");
  } finally {
    if (run === runtime.ownedRun) {
      state.ownedLoading = false;
      renderOwned();
    }
  }
}

export async function scanRoles(force = false): Promise<void> {
  const address = state.rolesAddress.trim();
  const status = byId<HTMLElement>("roles-status");
  if (!isValidAccountId(address)) {
    setStatus(status, "Enter a valid NEAR account id.", "error");
    return;
  }

  state.rolesAddress = address;
  const run = ++runtime.rolesRun;
  state.rolesLoading = true;
  state.rolesScanned = false;
  state.memberships = [];
  renderRoles();
  updateUrl();
  setStatus(status, `Reading roles for ${address} across the factory registry…`);

  try {
    const [records, blockTimestamp] = await Promise.all([
      getRegistry(force),
      latestBlockTimestamp(),
    ]);
    const memberships = await loadMembershipContracts(
      state.config.rpcUrl,
      records,
      address,
    );
    if (run !== runtime.rolesRun) return;
    state.blockTimestampNs = blockTimestamp;
    state.memberships = memberships;
    state.rolesScanned = true;
    renderRoles();
    const roleCount = memberships.reduce((total, item) => total + item.memberships.length, 0);
    setStatus(
      status,
      records.length === 0
        ? "The factory registry is empty. Deploy a contract first."
        : `Found ${roleCount} role membership(s) across ${memberships.length} contract(s).`,
      "ok",
    );
  } catch (error) {
    if (run !== runtime.rolesRun) return;
    state.memberships = [];
    state.rolesScanned = true;
    renderRoles();
    setStatus(status, walletErrorMessage(error), "error");
  } finally {
    if (run === runtime.rolesRun) {
      state.rolesLoading = false;
      renderRoles();
    }
  }
}

export async function scanAccess(force = false): Promise<void> {
  const status = byId<HTMLElement>("access-status");
  state.oidcError = "";
  let accountId = state.oidcSession?.accountId ?? state.walletAccount ?? "";
  if (!accountId) {
    state.accessMemberships = [];
    state.accessScanned = false;
    renderAccess();
    setStatus(status, "Sign in with OIDC or connect a wallet to inspect access.", "error");
    return;
  }
  if (!isValidAccountId(accountId)) {
    state.accessMemberships = [];
    state.accessScanned = false;
    renderAccess();
    setStatus(status, "The signed-in account id is not valid.", "error");
    return;
  }

  const run = ++runtime.accessRun;
  state.accessLoading = true;
  state.accessScanned = false;
  state.accessMemberships = [];
  renderAccess();
  setStatus(status, `Reading on-chain roles for ${accountId}…`);

  try {
    if (state.oidcSession) {
      try {
        state.oidcSession = await refreshOidcSession(state.oidcSession);
        accountId = state.oidcSession.accountId;
      } catch (error) {
        state.oidcError = `Could not refresh the OIDC session: ${walletErrorMessage(error)}`;
      }
    }

    const [records, blockTimestamp] = await Promise.all([
      getRegistry(force),
      latestBlockTimestamp(),
    ]);
    const memberships = await loadMembershipContracts(state.config.rpcUrl, records, accountId);
    if (run !== runtime.accessRun) return;
    state.blockTimestampNs = blockTimestamp;
    state.accessMemberships = memberships;
    state.accessScanned = true;
    renderAccess();
    const roleCount = memberships.reduce((total, item) => total + item.memberships.length, 0);
    setStatus(
      status,
      records.length === 0
        ? "The factory registry is empty. Deploy a contract first."
        : `${accountId} holds ${roleCount} role(s) across ${memberships.length} contract(s).`,
      "ok",
    );
  } catch (error) {
    if (run !== runtime.accessRun) return;
    state.accessMemberships = [];
    state.accessScanned = true;
    renderAccess();
    setStatus(status, walletErrorMessage(error), "error");
  } finally {
    if (run === runtime.accessRun) {
      state.accessLoading = false;
      renderAccess();
    }
  }
}

export async function handleOidcLogin(): Promise<void> {
  const status = byId<HTMLElement>("access-status");
  if (!oidcConfigured(state.config)) {
    setStatus(
      status,
      "OIDC is not configured. Set PUBLIC_OIDC_ISSUER and PUBLIC_OIDC_CLIENT_ID at build time.",
      "error",
    );
    return;
  }
  try {
    setStatus(status, "Opening the OIDC provider…");
    await beginOidcLogin(state.config);
  } catch (error) {
    setStatus(status, `OIDC sign-in failed: ${walletErrorMessage(error)}`, "error");
  }
}

export function handleOidcLogout(): void {
  clearOidcSession();
  state.oidcSession = null;
  state.oidcError = "";
  state.accessMemberships = [];
  state.accessScanned = false;
  renderAccess();
  setStatus(byId<HTMLElement>("access-status"), "Signed out of OIDC.", "ok");
}

export async function handleFund(button: HTMLButtonElement): Promise<void> {
  const contractId = button.dataset.contract ?? "";
  const snapshot = findSnapshot(contractId);
  const status = activeStatus();
  if (!snapshot) {
    setStatus(status, "Contract data is not loaded yet. Refresh and try again.", "error");
    return;
  }
  if (!state.config.salaryTokenId || snapshot.salaryToken !== state.config.salaryTokenId) {
    setStatus(status, "This contract does not use the configured demo salary token.", "error");
    return;
  }

  let amount: bigint;
  try {
    const decimals =
      snapshot.token.fallback && snapshot.salaryToken === state.config.salaryTokenId
        ? state.config.salaryTokenDecimals
        : snapshot.token.decimals;
    amount = parseUnits(state.config.demoFundAmount, decimals);
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
    return;
  }
  if (amount <= 0n) {
    setStatus(status, "The demo funding amount must be positive.", "error");
    return;
  }

  await executeWrite(
    status,
    `Fund ${contractId} with ${state.config.demoFundAmount} ${snapshot.token.symbol}`,
    state.config.salaryTokenId,
    "mint",
    { account_id: contractId, amount: amount.toString() },
  );
}

export function handleRoleAccess(form: HTMLFormElement): void {
  const contractId = form.dataset.contract ?? "";
  const data = new FormData(form);
  const role = str(data.get("role")).trim();
  if (!role) {
    showNotice("Choose a role to configure.", "error");
    return;
  }
  const labels = normalizeLabels(str(data.get("labels")));
  state.accessMap = setRoleLabels(
    localStorage,
    state.config.network,
    contractId,
    role,
    labels,
  );
  showNotice(
    labels.length === 0
      ? `Cleared OIDC access labels for ${role}.`
      : `Saved ${labels.length} OIDC access label(s) for ${role}.`,
  );
  renderOwned();
  renderAccess();
}

export function selectTab(tab: Tab): void {
  state.tab = tab;
  renderTabs();
  updateUrl();
  if (tab === "contracts" && state.ownedAddress && !state.ownedScanned && !state.ownedLoading) {
    void scanOwned();
  }
  if (tab === "roles" && state.rolesAddress && !state.rolesScanned && !state.rolesLoading) {
    void scanRoles();
  }
  if (tab === "access" && !state.accessScanned && !state.accessLoading) {
    void scanAccess();
  }
}

export function updateWallet(accountId: string | null): void {
  state.walletAccount = accountId;
  renderWallet();

  if (accountId) {
    const deployOwner = byId<HTMLInputElement>("deploy-owner");
    if (!deployOwner.value) deployOwner.value = accountId;

    if (!state.ownedAddress) {
      state.ownedAddress = accountId;
      byId<HTMLInputElement>("owned-address").value = accountId;
    }
    if (!state.rolesAddress) {
      state.rolesAddress = accountId;
      byId<HTMLInputElement>("roles-address").value = accountId;
    }
  }

  // Admin controls depend on the connected account.
  renderOwned();
  renderRoles();
  renderAccess();

  if (
    accountId &&
    state.tab === "contracts" &&
    state.ownedAddress &&
    !state.ownedScanned &&
    !state.ownedLoading
  ) {
    void scanOwned();
  }
  if (
    accountId &&
    state.tab === "roles" &&
    state.rolesAddress &&
    !state.rolesScanned &&
    !state.rolesLoading
  ) {
    void scanRoles();
  }
  if (
    accountId &&
    state.tab === "access" &&
    !state.oidcSession &&
    !state.accessScanned &&
    !state.accessLoading
  ) {
    void scanAccess();
  }
}

async function executeWrite(
  status: HTMLElement,
  description: string,
  contractId: string,
  methodName: string,
  args: Record<string, unknown>,
  options: { deposit?: bigint; gas?: bigint } = {},
): Promise<boolean> {
  if (!state.wallet) {
    setStatus(status, "Connect a NEAR wallet first.", "error");
    return false;
  }
  if (!state.walletAccount) {
    setStatus(status, "Connect a NEAR wallet first.", "error");
    return false;
  }

  setStatus(status, `${description} — confirm in your wallet…`);
  try {
    const outcome = await signAndSendFunctionCall(state.wallet, state.walletAccount, {
      contractId,
      methodName,
      args,
      deposit: options.deposit ?? 0n,
      gas: options.gas ?? WRITE_GAS,
    });
    const hash = transactionHash(outcome);
    if (hash) {
      setStatusLink(status, `${description} submitted.`, transactionExplorerUrl(state.config.explorerUrl, hash));
    } else {
      setStatus(
        status,
        `${description} submitted. If your wallet opened another tab, return here after approving.`,
        "ok",
      );
    }
    runtime.registryCache = null;
    window.setTimeout(() => {
      void refreshFactory();
      if (state.tab === "contracts" && state.ownedAddress) void scanOwned(true);
      if (state.tab === "roles" && state.rolesAddress) void scanRoles(true);
      if (state.tab === "access") void scanAccess(true);
    }, 1_800);
    return true;
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
    return false;
  }
}

export async function handleDeploy(form: HTMLFormElement): Promise<void> {
  const status = byId<HTMLElement>("deploy-status");
  if (!state.config.factoryContractId) {
    setStatus(status, "The factory contract is not configured.", "error");
    return;
  }
  if (!state.factoryInfo || state.factoryInfo.codeLen === 0) {
    setStatus(status, "The factory is not ready to deploy contracts.", "error");
    return;
  }
  if (!state.wallet || !state.walletAccount) {
    setStatus(status, "Connect a NEAR wallet to pay for the deployment.", "error");
    return;
  }

  const data = new FormData(form);
  const name = str(data.get("name")).trim();
  const owner = str(data.get("owner")).trim();
  const token = str(data.get("token")).trim();
  const nameError = subaccountNameError(name, state.config.factoryContractId);
  if (nameError) {
    setStatus(status, nameError, "error");
    return;
  }
  if (!isValidAccountId(owner)) {
    setStatus(status, "Enter a valid contract owner account id.", "error");
    return;
  }
  if (!isValidAccountId(token)) {
    setStatus(status, "Enter a valid NEP-141 salary token account id.", "error");
    return;
  }

  let interval: bigint;
  let deposit: bigint;
  try {
    interval = parseInterval(
      str(data.get("intervalValue")),
      str(data.get("intervalUnit")),
    );
    deposit = parseNear(str(data.get("deposit")));
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
    return;
  }

  const required = BigInt(state.factoryInfo.requiredDeposit);
  if (deposit < required) {
    setStatus(
      status,
      `Attach at least ${formatNear(required, 4)} to cover the wasm and child state.`,
      "error",
    );
    return;
  }

  const child = childAccountId(name, state.config.factoryContractId);
  setStatus(status, `Deploying ${child} — confirm in your wallet…`);
  try {
    const outcome = await signAndSendFunctionCall(state.wallet, state.walletAccount, {
      contractId: state.config.factoryContractId,
      methodName: "deploy",
      args: {
        name,
        owner_id: owner,
        salary_token: token,
        payout_interval_ns: interval.toString(),
      },
      deposit,
      gas: DEPLOY_GAS,
    });
    const hash = transactionHash(outcome);
    if (hash) {
      setStatusLink(
        status,
        `Deployment of ${child} submitted. It appears after finality.`,
        transactionExplorerUrl(state.config.explorerUrl, hash),
      );
    } else {
      setStatus(
        status,
        `Deployment of ${child} submitted. Approve it in your wallet; this page will refresh when you return.`,
        "ok",
      );
    }

    if (owner === state.walletAccount) {
      const favorites = loadFavorites(localStorage, state.config.network, owner);
      favorites.add(child);
      saveFavorites(localStorage, state.config.network, owner, favorites);
    }
    showNotice(`Deployment of ${child} submitted.`);
    runtime.registryCache = null;
    form.reset();
    window.setTimeout(() => {
      void refreshFactory();
      if (state.ownedAddress === owner) void scanOwned(true);
    }, 3_000);
  } catch (error) {
    setStatus(status, walletErrorMessage(error), "error");
  }
}

export async function handleAdminForm(kind: string, form: HTMLFormElement): Promise<void> {
  const contractId = form.dataset.contract ?? "";
  const snapshot = findSnapshot(contractId);
  const status = activeStatus();
  if (!snapshot) {
    setStatus(status, "Contract data is not loaded yet. Refresh and try again.", "error");
    return;
  }

  const data = new FormData(form);
  let ok = false;

  if (kind === "mint" || kind === "burn") {
    const role = form.dataset.role ?? "";
    const account = str(data.get("account")).trim();
    if (!isValidAccountId(account)) {
      setStatus(status, "Enter a valid account id.", "error");
      return;
    }
    if (kind === "burn") {
      const confirmed = window.confirm(
        `Burn ${role} from ${account}? Accrued salary is paid to ${account} first; if that transfer fails the membership stays.`,
      );
      if (!confirmed) return;
    }
    ok = await executeWrite(
      status,
      `${kind === "mint" ? "Mint" : "Burn"} ${role} ${kind === "mint" ? "to" : "from"} ${account}`,
      contractId,
      kind === "mint" ? "mint_role" : "burn_role",
      kind === "mint"
        ? { account_id: account, role }
        : { account_id: account, role, pay_pending: true },
      kind === "mint" ? { deposit: BigInt(snapshot.requiredMintDeposit) } : {},
    );
  } else if (kind === "role-salary") {
    const role = str(data.get("role")).trim();
    if (!role) {
      setStatus(status, "Enter a role name.", "error");
      return;
    }
    let salary: bigint;
    try {
      salary = parseUnits(str(data.get("amount")), snapshot.token.decimals);
    } catch (error) {
      setStatus(status, walletErrorMessage(error), "error");
      return;
    }
    ok = await executeWrite(status, `Save role ${role}`, contractId, "set_role_salary", {
      role,
      salary: salary.toString(),
    });
  } else if (kind === "role-owner") {
    const role = str(data.get("role")).trim();
    const owner = str(data.get("owner")).trim();
    if (!role) {
      setStatus(status, "Enter a role name.", "error");
      return;
    }
    if (owner && !isValidAccountId(owner)) {
      setStatus(status, "Enter a valid role owner, or leave it blank.", "error");
      return;
    }
    ok = await executeWrite(status, `Set owner for ${role}`, contractId, "set_role_owner", {
      role,
      owner: owner || null,
    });
  } else if (kind === "payout") {
    const token = str(data.get("token")).trim();
    if (!isValidAccountId(token)) {
      setStatus(status, "Enter a valid salary token account id.", "error");
      return;
    }
    let interval: bigint;
    try {
      interval = parseInterval(
        str(data.get("intervalValue")),
        str(data.get("intervalUnit")),
      );
    } catch (error) {
      setStatus(status, walletErrorMessage(error), "error");
      return;
    }
    ok = await executeWrite(status, "Update payroll configuration", contractId, "set_payout_config", {
      salary_token: token,
      payout_interval_ns: interval.toString(),
    });
  } else if (kind === "transfer-owner") {
    const owner = str(data.get("owner")).trim();
    if (!isValidAccountId(owner)) {
      setStatus(status, "Enter a valid new owner account id.", "error");
      return;
    }
    if (!window.confirm(`Transfer contract ownership to ${owner}?`)) return;
    ok = await executeWrite(status, `Transfer ownership to ${owner}`, contractId, "set_owner", {
      new_owner_id: owner,
    });
  }

  if (ok) {
    form.reset();
    showNotice("Transaction submitted.");
  }
}

export async function handleClaim(button: HTMLButtonElement): Promise<void> {
  const contractId = button.dataset.contract ?? "";
  const account = button.dataset.account ?? "";
  const role = button.dataset.role;
  if (!isValidAccountId(contractId) || !isValidAccountId(account)) {
    showNotice("Invalid claim target.", "error");
    return;
  }

  const status = byId<HTMLElement>("roles-status");
  await executeWrite(
    status,
    role ? `Claim salary for ${role}` : "Claim all salary",
    contractId,
    role ? "claim_role" : "claim",
    role ? { account_id: account, role } : { account_id: account },
    { gas: CLAIM_GAS },
  );
}

export function handleFavorite(button: HTMLButtonElement): void {
  const contractId = button.dataset.contract ?? "";
  if (!isValidAccountId(state.ownedAddress)) {
    showNotice("Enter an owner address before favoriting contracts.", "error");
    return;
  }
  state.favorites = toggleFavorite(
    localStorage,
    state.config.network,
    state.ownedAddress,
    contractId,
  );
  const active = state.favorites.has(contractId);
  showNotice(active ? "Added to favorites." : "Removed from favorites.");
  renderOwned();
}

export async function startWallet(): Promise<void> {
  if (!state.config.factoryContractId) {
    renderWallet();
  }
  try {
    const connector = initWallet(state.config);
    state.connector = connector;

    connector.on("wallet:signIn", ({ wallet, accounts, success }) => {
      if (!success) return;
      state.wallet = wallet;
      updateWallet(accounts[0]?.accountId ?? null);
      showNotice("Wallet connected.");
    });
    connector.on("wallet:signOut", () => {
      state.wallet = null;
      updateWallet(null);
      showNotice("Wallet disconnected.");
    });

    renderWallet();

    const restored = await restoreWallet(connector);
    if (restored) {
      state.wallet = restored.wallet;
      updateWallet(restored.accountId);
    }
  } catch (error) {
    renderWallet();
    showNotice(`Wallet connector failed to load: ${walletErrorMessage(error)}`, "error");
  }
}
