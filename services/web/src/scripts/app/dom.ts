import { runtime, type Tone } from "./state";

export function byId<T extends HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element #${id}`);
  return element as T;
}

export function str(value: FormDataEntryValue | null | undefined): string {
  return typeof value === "string" ? value : "";
}

export function escapeHtml(value: unknown): string {
  return String(value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function setStatus(element: HTMLElement, message: string, tone: Tone = "muted"): void {
  element.className = tone === "muted" ? "status" : `status ${tone}`;
  element.textContent = message;
}

export function setStatusLink(element: HTMLElement, message: string, url: string): void {
  element.className = "status ok";
  element.innerHTML = `${escapeHtml(message)} <a href="${escapeHtml(url)}" target="_blank" rel="noreferrer">View transaction ↗</a>`;
}

export function showNotice(message: string, tone: "ok" | "error" = "ok"): void {
  const notice = byId<HTMLElement>("global-notice");
  notice.hidden = false;
  notice.className = `notice ${tone}`;
  notice.textContent = message;
  if (runtime.noticeTimer) window.clearTimeout(runtime.noticeTimer);
  if (tone === "ok") {
    runtime.noticeTimer = window.setTimeout(() => {
      notice.hidden = true;
    }, 7_000);
  }
}

/**
 * Disables a submit button, marks it busy for assistive technology and restores its label when the
 * task settles. The task itself reports progress through the status line.
 */
export async function withBusy(
  button: HTMLButtonElement | null,
  pendingLabel: string,
  task: () => Promise<void>,
): Promise<void> {
  if (!button) {
    await task();
    return;
  }
  const label = button.textContent ?? "";
  button.disabled = true;
  button.setAttribute("aria-busy", "true");
  if (pendingLabel) button.textContent = pendingLabel;
  try {
    await task();
  } finally {
    button.disabled = false;
    button.removeAttribute("aria-busy");
    button.textContent = label;
  }
}
