import { childAccountId, subaccountNameError } from "../../lib/accounts";
import { walletErrorMessage } from "../../lib/wallet";
import {
  handleAdminForm,
  handleClaim,
  handleDeploy,
  handleFavorite,
  handleFund,
  handleOidcLogin,
  handleOidcLogout,
  handleRoleAccess,
  scanAccess,
  scanOwned,
  scanRoles,
  selectTab,
} from "./actions";
import { byId, showNotice, withBusy } from "./dom";
import { state } from "./state";
import { renderOwned } from "./views";

export function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const tab = button.dataset.tab;
      if (tab === "deploy" || tab === "contracts" || tab === "roles" || tab === "access") {
        selectTab(tab);
      }
    });
  });

  byId<HTMLButtonElement>("wallet-button").addEventListener("click", async () => {
    if (!state.connector) {
      showNotice("Wallet connector is still loading. Try again in a moment.", "error");
      return;
    }
    try {
      await state.connector.connect();
    } catch (error) {
      const message = walletErrorMessage(error);
      // Closing the wallet picker is not an error worth surfacing.
      if (!/user rejected|cancel/i.test(message)) {
        showNotice(message, "error");
      }
    }
  });

  byId<HTMLButtonElement>("wallet-disconnect").addEventListener("click", async () => {
    if (!state.connector) return;
    try {
      await state.connector.disconnect(state.wallet ?? undefined);
    } catch (error) {
      showNotice(walletErrorMessage(error), "error");
    }
  });

  byId<HTMLButtonElement>("owned-use-wallet").addEventListener("click", () => {
    if (!state.walletAccount) {
      showNotice("Connect a wallet first.", "error");
      return;
    }
    state.ownedAddress = state.walletAccount;
    byId<HTMLInputElement>("owned-address").value = state.walletAccount;
    void scanOwned();
  });

  byId<HTMLButtonElement>("roles-use-wallet").addEventListener("click", () => {
    if (!state.walletAccount) {
      showNotice("Connect a wallet first.", "error");
      return;
    }
    state.rolesAddress = state.walletAccount;
    byId<HTMLInputElement>("roles-address").value = state.walletAccount;
    void scanRoles();
  });

  byId<HTMLButtonElement>("roles-refresh").addEventListener("click", () => {
    void scanRoles(true);
  });

  byId<HTMLButtonElement>("oidc-login").addEventListener("click", () => {
    void handleOidcLogin();
  });

  byId<HTMLButtonElement>("oidc-logout").addEventListener("click", () => {
    handleOidcLogout();
  });

  byId<HTMLButtonElement>("access-refresh").addEventListener("click", () => {
    void scanAccess(true);
  });

  byId<HTMLInputElement>("favorites-only").addEventListener("change", (event) => {
    state.favoritesOnly = (event.target as HTMLInputElement).checked;
    const url = new URL(window.location.href);
    if (state.favoritesOnly) url.searchParams.set("favorites", "1");
    else url.searchParams.delete("favorites");
    window.history.replaceState({}, "", url);
    renderOwned();
  });

  byId<HTMLInputElement>("deploy-name").addEventListener("input", () => {
    const name = byId<HTMLInputElement>("deploy-name").value.trim();
    const preview = byId<HTMLElement>("deploy-preview");
    if (!state.config.factoryContractId) {
      preview.textContent = "Configure the factory contract to deploy.";
      return;
    }
    const error = subaccountNameError(name, state.config.factoryContractId);
    preview.textContent = error
      ? error
      : `Child account: ${childAccountId(name, state.config.factoryContractId)}`;
    preview.className = error ? "preview status error" : "preview";
  });

  document.addEventListener("submit", (event) => {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    const submit = form.querySelector<HTMLButtonElement>('button[type="submit"]');

    if (form.id === "deploy-form") {
      event.preventDefault();
      void withBusy(submit, "Deploying…", () => handleDeploy(form));
      return;
    }
    if (form.id === "owned-form") {
      event.preventDefault();
      state.ownedAddress = byId<HTMLInputElement>("owned-address").value.trim();
      void withBusy(submit, "Scanning…", () => scanOwned());
      return;
    }
    if (form.id === "roles-form") {
      event.preventDefault();
      state.rolesAddress = byId<HTMLInputElement>("roles-address").value.trim();
      void withBusy(submit, "Scanning…", () => scanRoles());
      return;
    }
    const kind = form.dataset.form;
    if (kind === "role-access") {
      event.preventDefault();
      handleRoleAccess(form);
      return;
    }
    if (kind) {
      event.preventDefault();
      const label = kind === "mint" ? "Minting…" : kind === "burn" ? "Burning…" : "Submitting…";
      void withBusy(submit, label, () => handleAdminForm(kind, form));
    }
  });

  document.addEventListener("change", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLSelectElement) || !target.matches("[data-access-role]")) return;
    const form = target.closest<HTMLFormElement>("form");
    const input = form?.querySelector<HTMLInputElement>('input[name="labels"]');
    if (input) input.value = target.selectedOptions[0]?.dataset.labels ?? "";
  });

  document.addEventListener("click", (event) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;
    const action = target.closest<HTMLElement>("[data-action]");
    if (!action) return;
    const button = action as HTMLButtonElement;
    const name = action.dataset.action;
    if (name === "favorite") {
      handleFavorite(button);
    } else if (name === "fund") {
      void withBusy(button, "Funding…", () => handleFund(button));
    } else if (name === "claim" || name === "claim-all") {
      void withBusy(button, "Claiming…", () => handleClaim(button));
    }
  });
}
