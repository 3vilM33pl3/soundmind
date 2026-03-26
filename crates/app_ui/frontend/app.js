// @ts-check

import { createApi, DEFAULT_BACKEND_URL } from "./lib/api.js";
import {
  clearManualQuestionSelection,
  createState,
  currentActionSelection,
  promoteManualQuestionSelection,
  RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY,
} from "./lib/app_state.js";
import { createElements } from "./lib/dom.js";
import { renderDisconnected, runWithButtonFeedback, applyTheme } from "./lib/ui.js";
import { arrayBufferToBase64, preferredTheme } from "./lib/utils.js";
import {
  renderSettings,
  renderPrimingDocuments,
  buildSettingsPayload,
  renderLlmModelOptions,
  renderLlmModelsForProvider,
} from "./components/settings.js";
import { renderSessions, renderSessionDetail } from "./components/history.js";
import {
  clearTranscriptSelection,
  refreshCurrentSelectionTarget,
  renderSelectionState,
  renderTranscriptPanel,
  updateTranscriptSelection,
} from "./components/transcript.js";
import { renderSnapshot as renderStatusSnapshot } from "./components/status.js";

const backendUrl = window.localStorage.getItem("soundmind.backendUrl") || DEFAULT_BACKEND_URL;
const APP_VERSION = window.SOUNDMIND_VERSION || "0.2.1";

const state = createState();
const els = createElements();
const api = createApi(backendUrl);

const app = {
  api,
  state,
  els,
  renderDisconnected(error) {
    renderDisconnected(els, backendUrl, error);
  },
  renderSnapshot(snapshot) {
    renderStatusSnapshot(state, els, backendUrl, snapshot, { app });
  },
  async refreshHistory(force = false) {
    const now = Date.now();
    if (!force && now - state.lastHistoryRefreshAt < 6000) {
      return;
    }

    state.sessions = await api.fetchSessions();
    state.lastHistoryRefreshAt = now;
    const restoredExists = state.restoredTranscriptSessionId
      ? state.sessions.some((session) => session.id === state.restoredTranscriptSessionId)
      : false;

    if (state.restoredTranscriptSessionId && restoredExists) {
      state.selectedSessionId = state.restoredTranscriptSessionId;
    } else if (!state.selectedSessionId && state.sessions.length) {
      state.selectedSessionId = state.sessions[0].id;
    }

    renderSessions(state, els, app);

    if (state.selectedSessionId) {
      const exists = state.sessions.some((session) => session.id === state.selectedSessionId);
      if (!exists) {
        state.selectedSessionId = state.sessions[0]?.id || null;
      }
    }

    if (state.selectedSessionId) {
      state.selectedSession = await api.fetchSessionDetail(state.selectedSessionId);
    } else {
      state.selectedSession = null;
    }
    renderSessionDetail(state, els, app);
    renderTranscriptPanel(state, els, app);
  },
  async refreshSettings(force = false) {
    const now = Date.now();
    if (!force && now - state.lastSettingsRefreshAt < 12000) {
      return;
    }

    const [settings, models] = await Promise.all([api.fetchSettings(), api.fetchLlmModels()]);
    state.lastSettingsRefreshAt = now;
    if (
      !state.availableLlmModels.length ||
      JSON.stringify(state.availableLlmModels) !== JSON.stringify(models)
    ) {
      renderLlmModelOptions(state, els, models);
    }
    if (!state.settings || JSON.stringify(state.settings) !== JSON.stringify(settings)) {
      renderSettings(state, els, settings);
    }
    renderLlmModelsForProvider(state, els, settings.llm_provider, settings.llm_model);
  },
  async refreshPrimingDocuments(force = false) {
    const now = Date.now();
    if (!force && now - state.lastPrimingRefreshAt < 12000) {
      return;
    }

    state.primingDocuments = await api.fetchPrimingDocuments();
    state.lastPrimingRefreshAt = now;
    renderPrimingDocuments(state, els, app);
  },
  async selectSession(sessionId) {
    state.selectedSessionId = sessionId;
    state.selectedSession = await api.fetchSessionDetail(sessionId);
    state.restoredTranscriptSessionId = sessionId;
    window.localStorage.setItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY, sessionId);
    renderSessions(state, els, app);
    renderSessionDetail(state, els, app);
    renderTranscriptPanel(state, els, app);
  },
  async handleExport(sessionId, format) {
    try {
      const payload = await api.exportSession(sessionId, format);
      const extension = format === "markdown" ? "md" : "json";
      const mimeType = format === "markdown" ? "text/markdown" : "application/json";
      const blob = new Blob([payload], { type: mimeType });
      const url = URL.createObjectURL(blob);
      const link = document.createElement("a");
      link.href = url;
      link.download = `soundmind-session-${sessionId}.${extension}`;
      link.click();
      URL.revokeObjectURL(url);
      els.settingsNote.textContent = `Exported session ${sessionId} as ${extension}.`;
    } catch (error) {
      app.renderDisconnected(error);
    }
  },
};

function actionLabels(action) {
  switch (action) {
    case "Start":
      return { pending: "Starting...", success: "Capture On", error: "Start Failed" };
    case "Stop":
      return { pending: "Stopping...", success: "Capture Off", error: "Stop Failed" };
    case "PauseCloud":
      return { pending: "Pausing...", success: "Cloud Paused", error: "Pause Failed" };
    case "ResumeCloud":
      return { pending: "Resuming...", success: "Cloud Live", error: "Resume Failed" };
    case "AnswerLastQuestion":
      return { pending: "Answering...", success: "Answer Ready", error: "Answer Failed" };
    case "SummariseLastMinute":
      return { pending: "Summarising...", success: "Summary Ready", error: "Summary Failed" };
    case "CommentCurrentTopic":
      return { pending: "Commenting...", success: "Comment Ready", error: "Comment Failed" };
    case "AnswerSelection":
      return { pending: "Answering...", success: "Answer Ready", error: "Answer Failed" };
    case "SummariseSelection":
      return { pending: "Summarizing...", success: "Summary Ready", error: "Summary Failed" };
    case "CommentSelection":
      return { pending: "Commenting...", success: "Comment Ready", error: "Comment Failed" };
    default:
      return { pending: "Working...", success: "Done", error: "Failed" };
  }
}

function resolvePrimaryAction(defaultAction) {
  const selection = currentActionSelection(state);
  if (!selection) {
    return defaultAction;
  }

  switch (defaultAction) {
    case "AnswerLastQuestion":
      return { AnswerSelection: selection };
    case "SummariseLastMinute":
      return { SummariseSelection: selection };
    case "CommentCurrentTopic":
      return { CommentSelection: selection };
    default:
      return defaultAction;
  }
}

function actionKey(action) {
  return typeof action === "string" ? action : Object.keys(action)[0];
}

async function handleSettingsSave() {
  const previousSettings = state.settings;
  const nextSettings = buildSettingsPayload(els);
  try {
    const saved = await api.putSettings(nextSettings);
    state.lastSettingsRefreshAt = Date.now();
    renderSettings(state, els, saved);
    if (!previousSettings || previousSettings.default_mode !== saved.default_mode) {
      await api.sendAction({ SetMode: saved.default_mode });
    }
    if (!previousSettings || previousSettings.auto_start_cloud !== saved.auto_start_cloud) {
      await api.sendAction(saved.auto_start_cloud ? "ResumeCloud" : "PauseCloud");
    }
    if (
      !previousSettings ||
      previousSettings.llm_provider !== saved.llm_provider ||
      previousSettings.llm_model !== saved.llm_model
    ) {
      els.settingsNote.textContent = `LLM switched to ${saved.llm_provider}:${saved.llm_model}.`;
    }
    const snapshot = await api.fetchHealth();
    app.renderSnapshot(snapshot);
    if (
      previousSettings &&
      previousSettings.llm_provider === saved.llm_provider &&
      previousSettings.llm_model === saved.llm_model
    ) {
      els.settingsNote.textContent = "Settings saved and applied.";
    }
  } catch (error) {
    app.renderDisconnected(error);
  }
}

async function handlePurge() {
  const retentionDays = Number.parseInt(els.retentionDays.value || "0", 10) || 0;
  if (retentionDays === 0) {
    els.settingsNote.textContent =
      "Retention is set to 0, so purge will not delete anything automatically.";
    return false;
  }
  const confirmed = window.confirm(
    `Delete stored sessions older than ${retentionDays} days? This does not affect the running session.`,
  );
  if (!confirmed) {
    return false;
  }

  try {
    const result = await api.purgeHistory();
    await app.refreshHistory(true);
    els.settingsNote.textContent = `Purged ${result.deleted} stale sessions.`;
    return true;
  } catch (error) {
    app.renderDisconnected(error);
    throw error;
  }
}

async function handlePrimingUpload() {
  const files = Array.from(els.primingFileInput.files || []);
  if (!files.length) {
    els.settingsNote.textContent = "Choose one or more documents to upload first.";
    return false;
  }

  for (const file of files) {
    const buffer = await file.arrayBuffer();
    const payload = {
      file_name: file.name,
      mime_type: file.type || null,
      content_base64: arrayBufferToBase64(buffer),
    };
    await api.uploadPrimingDocument(payload);
  }

  els.primingFileInput.value = "";
  await app.refreshPrimingDocuments(true);
  els.settingsNote.textContent = `Uploaded ${files.length} priming document${files.length === 1 ? "" : "s"}.`;
  return true;
}

async function handleClearAll() {
  clearTranscriptSelection(state, els);
  clearManualQuestionSelection(state);
  await api.sendAction("ClearCurrentView");
  const snapshot = await api.fetchHealth();
  app.renderSnapshot(snapshot);
  return true;
}

function bindEventHandlers() {
  document.querySelectorAll(".hero-actions [data-action]").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await runWithButtonFeedback(
          button,
          async () => {
            await api.sendAction(button.dataset.action);
            const snapshot = await api.fetchHealth();
            app.renderSnapshot(snapshot);
          },
          actionLabels(button.dataset.action),
        );
      } catch (error) {
        app.renderDisconnected(error);
      }
    });
  });

  [
    [els.actionAnswerButton, "AnswerLastQuestion"],
    [els.actionSummaryButton, "SummariseLastMinute"],
    [els.actionCommentButton, "CommentCurrentTopic"],
  ].forEach(([button, defaultAction]) => {
    button.addEventListener("pointerdown", () => {
      refreshCurrentSelectionTarget(state, els);
    });
    button.addEventListener("click", async () => {
      refreshCurrentSelectionTarget(state, els);
      const action = resolvePrimaryAction(defaultAction);
      try {
        await runWithButtonFeedback(
          button,
          async () => {
            await api.sendAction(action);
            const snapshot = await api.fetchHealth();
            app.renderSnapshot(snapshot);
            if (actionKey(action) === "AnswerSelection") {
              promoteManualQuestionSelection(state, action.AnswerSelection);
              renderTranscriptPanel(state, els, app);
            }
          },
          actionLabels(actionKey(action)),
        );
      } catch (error) {
        app.renderDisconnected(error);
      }
    });
  });

  els.clearSelectionButton.addEventListener("click", () => {
    clearTranscriptSelection(state, els);
  });

  els.transcriptReturnLive?.addEventListener("click", () => {
    state.restoredTranscriptSessionId = null;
    window.localStorage.removeItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY);
    clearTranscriptSelection(state, els);
    clearManualQuestionSelection(state);
    renderTranscriptPanel(state, els, app);
  });

  els.clearAllButton.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        els.clearAllButton,
        () => handleClearAll(),
        { pending: "Clearing...", success: "Cleared", error: "Clear Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  document.addEventListener("selectionchange", () => {
    updateTranscriptSelection(state, els);
  });

  els.appVersion.textContent = `v${APP_VERSION}`;
  applyTheme(els, preferredTheme());
  els.themeToggle?.addEventListener("change", () => {
    applyTheme(els, els.themeToggle.checked ? "dark" : "light");
  });

  els.saveSettings.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        els.saveSettings,
        () => handleSettingsSave(),
        { pending: "Saving...", success: "Saved", error: "Save Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  els.llmProvider.addEventListener("change", async () => {
    renderLlmModelsForProvider(state, els, els.llmProvider.value, null);
    try {
      await handleSettingsSave();
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  els.llmModel.addEventListener("change", async () => {
    try {
      await handleSettingsSave();
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  els.saveAgentConfig.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        els.saveAgentConfig,
        () => handleSettingsSave(),
        { pending: "Saving...", success: "Saved", error: "Save Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  els.purgeHistory.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        els.purgeHistory,
        () => handlePurge(),
        { pending: "Purging...", success: "Purged", error: "Purge Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });

  els.uploadPrimingDocuments.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        els.uploadPrimingDocuments,
        () => handlePrimingUpload(),
        { pending: "Uploading...", success: "Uploaded", error: "Upload Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
      els.settingsNote.textContent =
        "Document upload failed. Text files work directly; PDF upload requires `pdftotext`.";
    }
  });
}

async function refreshLoop() {
  try {
    const snapshot = await api.fetchHealth();
    app.renderSnapshot(snapshot);
    await app.refreshSettings(false);
    await app.refreshPrimingDocuments(false);
    await app.refreshHistory(false);
  } catch (error) {
    app.renderDisconnected(error);
  } finally {
    window.setTimeout(refreshLoop, 300);
  }
}

els.privacyBackend.textContent = backendUrl;
bindEventHandlers();
refreshLoop();
