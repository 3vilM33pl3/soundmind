// @ts-check

import { applyTheme as applyThemeImpl } from "./utils.js";

export function setButtonVisualState(button, state, label = null) {
  if (!button) {
    return;
  }

  if (!button.dataset.defaultLabel) {
    button.dataset.defaultLabel = button.textContent.trim();
  }

  button.classList.remove("is-pending", "is-success", "is-error");
  if (state) {
    button.classList.add(`is-${state}`);
  }

  button.disabled = state === "pending";
  button.textContent = label || button.dataset.defaultLabel;
}

export async function runWithButtonFeedback(button, task, labels = {}) {
  const pendingLabel = labels.pending || "Working...";
  const successLabel = labels.success || "Done";
  const errorLabel = labels.error || "Failed";

  setButtonVisualState(button, "pending", pendingLabel);

  try {
    const result = await task();
    if (result === false) {
      setButtonVisualState(button, null);
      return result;
    }
    setButtonVisualState(button, "success", successLabel);
    window.setTimeout(() => setButtonVisualState(button, null), 900);
    return result;
  } catch (error) {
    setButtonVisualState(button, "error", errorLabel);
    window.setTimeout(() => setButtonVisualState(button, null), 1400);
    throw error;
  }
}

export function setChip(el, label, status) {
  el.textContent = label;
  el.classList.remove("ok", "warn", "error");
  if (status) {
    el.classList.add(status);
  }
}

export function setStatusBlock(block, valueEl, detailEl, value, detail, status) {
  if (valueEl) {
    valueEl.textContent = value;
  }
  if (detailEl) {
    detailEl.textContent = detail;
  }
  if (!block) {
    return;
  }
  block.classList.remove("ok", "warn", "error");
  if (status) {
    block.classList.add(status);
  }
}

export function classifyState(stateValue) {
  if (stateValue === "Capturing" || stateValue === "SttActive" || stateValue === "LlmActive") {
    return "ok";
  }
  if (stateValue === "Paused") {
    return "warn";
  }
  if (stateValue === "Error") {
    return "error";
  }
  return null;
}

export function renderDisconnected(els, backendUrl, error) {
  setStatusBlock(
    els.backendBlock,
    els.backendChip,
    els.backendDetail,
    "Backend disconnected",
    "The desktop app cannot reach the backend service.",
    "error",
  );
  setStatusBlock(
    els.captureBlock,
    els.captureChip,
    els.captureDetail,
    "Capture unavailable",
    "Local capture state is unavailable until the backend reconnects.",
    "warn",
  );
  setStatusBlock(
    els.cloudBlock,
    els.cloudChip,
    els.cloudDetail,
    "Cloud unknown",
    "Cloud status is unknown because the backend is unreachable.",
    "warn",
  );
  if (els.captureIndicator) {
    els.captureIndicator.dataset.state = "offline";
  }
  els.backendNote.textContent = `Cannot reach backend at ${backendUrl}. Start it with: cargo run -p app_backend (${error.message})`;
}

export function applyTheme(els, theme) {
  applyThemeImpl(els, theme);
}
