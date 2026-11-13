import { refreshFactory, restoreFromUrl, scanOwned, scanRoles, startWallet } from "./app/actions";
import { byId } from "./app/dom";
import { bindEvents } from "./app/events";
import { state } from "./app/state";
import { renderAll } from "./app/views";

async function init(): Promise<void> {
  byId<HTMLElement>("network-badge").textContent = state.config.network;
  bindEvents();
  restoreFromUrl();
  renderAll();

  if (state.tab === "contracts" && state.ownedAddress) {
    void scanOwned();
  }
  if (state.tab === "roles" && state.rolesAddress) {
    void scanRoles();
  }

  void refreshFactory();
  void startWallet();
}

void init();
