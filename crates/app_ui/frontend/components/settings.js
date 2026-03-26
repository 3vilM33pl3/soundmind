// @ts-check

import { escapeHtml } from "../lib/utils.js";
import { runWithButtonFeedback } from "../lib/ui.js";

export function renderSettings(state, els, settings) {
  state.settings = settings;
  els.settingsMode.value = settings.default_mode;
  els.openaiModel.value = settings.openai_model || "gpt-4o-mini";
  els.retentionDays.value = String(settings.retention_days);
  els.transcriptStorage.checked = settings.transcript_storage_enabled;
  els.autoStartCloud.checked = settings.auto_start_cloud;
  els.assistantInstruction.value = settings.assistant_instruction;
  els.privacyStorage.textContent = settings.transcript_storage_enabled
    ? settings.retention_days === 0
      ? "Transcripts are stored locally with no automatic expiry."
      : `Transcripts are stored locally for ${settings.retention_days} days.`
    : "Transcript storage is disabled for new segments.";
  els.settingsNote.textContent =
    "Settings loaded from the local SQLite store. Automatic cloud resume is off by default.";
}

export function buildSettingsPayload(els) {
  return {
    retention_days: Math.max(0, Number.parseInt(els.retentionDays.value || "0", 10) || 0),
    transcript_storage_enabled: els.transcriptStorage.checked,
    auto_start_cloud: els.autoStartCloud.checked,
    default_mode: els.settingsMode.value,
    openai_model: els.openaiModel.value.trim() || "gpt-4o-mini",
    assistant_instruction: els.assistantInstruction.value.trim(),
  };
}

export function renderPrimingDocuments(state, els, app) {
  if (!state.primingDocuments.length) {
    els.primingDocumentList.innerHTML =
      `<div class="empty-state">No priming documents uploaded yet.</div>`;
    return;
  }

  els.primingDocumentList.innerHTML = state.primingDocuments
    .map(
      (document) => `
        <article class="document-card">
          <div class="document-card-top">
            <div>
              <h4>${escapeHtml(document.file_name)}</h4>
              <div class="document-meta">${escapeHtml(document.mime_type)} • ${document.char_count} characters</div>
            </div>
            <button class="ghost" data-document-delete="${document.id}">Delete</button>
          </div>
          <div class="document-preview">${escapeHtml(document.preview_text || "No extracted text preview available.")}</div>
        </article>`,
    )
    .join("");

  document.querySelectorAll("[data-document-delete]").forEach((button) => {
    button.addEventListener("click", async () => {
      const { documentDelete } = button.dataset;
      if (!documentDelete) {
        return;
      }

      const confirmed = window.confirm("Delete this priming document?");
      if (!confirmed) {
        return;
      }

      try {
        await runWithButtonFeedback(
          button,
          async () => {
            await app.api.deletePrimingDocument(documentDelete);
            await app.refreshPrimingDocuments(true);
            els.settingsNote.textContent = "Priming document deleted.";
          },
          { pending: "Deleting...", success: "Deleted", error: "Delete Failed" },
        );
      } catch (error) {
        app.renderDisconnected(error);
      }
    });
  });
}

