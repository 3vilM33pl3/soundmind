// @ts-check

export const THEME_STORAGE_KEY = "soundmind.theme";

export function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

export function formatTime(value) {
  return new Date(value).toLocaleString();
}

export function normalizeTranscriptText(text) {
  return String(text || "").trim();
}

export function contentToBulletCandidates(lines) {
  const merged = lines.join(" ");
  return merged
    .split(/(?<=[.!?])\s+(?=[A-Z0-9])/)
    .map((line) => line.trim().replace(/^[*\-•]\s+/, ""))
    .filter((line) => line.length > 0);
}

export function preferredTheme() {
  const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (storedTheme === "light" || storedTheme === "dark") {
    return storedTheme;
  }

  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function applyTheme(els, theme) {
  const resolvedTheme = theme === "dark" ? "dark" : "light";
  document.documentElement.dataset.theme = resolvedTheme;
  if (els.themeToggle) {
    els.themeToggle.checked = resolvedTheme === "dark";
  }
  window.localStorage.setItem(THEME_STORAGE_KEY, resolvedTheme);
}

export function arrayBufferToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return window.btoa(binary);
}

