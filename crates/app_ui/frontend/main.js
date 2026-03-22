const DEFAULT_BACKEND_URL = "http://127.0.0.1:8765";
const backendUrl = window.localStorage.getItem("soundmind.backendUrl") || DEFAULT_BACKEND_URL;

const state = {
  snapshot: null,
  settings: null,
  sessions: [],
  selectedSessionId: null,
  selectedSession: null,
  lastSettingsRefreshAt: 0,
  lastHistoryRefreshAt: 0,
};

const els = {
  backendChip: document.querySelector("#backend-chip"),
  captureChip: document.querySelector("#capture-chip"),
  cloudChip: document.querySelector("#cloud-chip"),
  sttChip: document.querySelector("#stt-chip"),
  backendNote: document.querySelector("#backend-note"),
  questionBanner: document.querySelector("#question-banner"),
  partialBox: document.querySelector("#partial-box"),
  segmentList: document.querySelector("#segment-list"),
  assistantCard: document.querySelector("#assistant-card"),
  answerQuestionButton: document.querySelector("#answer-question-button"),
  errorList: document.querySelector("#error-list"),
  settingsMode: document.querySelector("#settings-mode"),
  retentionDays: document.querySelector("#retention-days"),
  transcriptStorage: document.querySelector("#transcript-storage"),
  autoStartCloud: document.querySelector("#auto-start-cloud"),
  saveSettings: document.querySelector("#save-settings"),
  purgeHistory: document.querySelector("#purge-history"),
  settingsNote: document.querySelector("#settings-note"),
  currentSink: document.querySelector("#current-sink"),
  monitorSource: document.querySelector("#monitor-source"),
  sttStatus: document.querySelector("#stt-status"),
  sessionId: document.querySelector("#session-id"),
  privacyPause: document.querySelector("#privacy-pause"),
  cloudPause: document.querySelector("#cloud-pause"),
  cloudAutoPause: document.querySelector("#cloud-auto-pause"),
  audioUploadActive: document.querySelector("#audio-upload-active"),
  privacyCapture: document.querySelector("#privacy-capture"),
  privacyCloud: document.querySelector("#privacy-cloud"),
  privacyStorage: document.querySelector("#privacy-storage"),
  privacyBackend: document.querySelector("#privacy-backend"),
  sessionList: document.querySelector("#session-list"),
  sessionDetail: document.querySelector("#session-detail"),
};

async function fetchJson(path, init = undefined) {
  const response = await fetch(`${backendUrl}${path}`, init);
  if (!response.ok) {
    throw new Error(`${path} returned ${response.status}`);
  }
  return response.json();
}

async function fetchHealth() {
  return fetchJson("/health");
}

async function fetchSettings() {
  return fetchJson("/settings");
}

async function fetchSessions() {
  return fetchJson("/sessions");
}

async function fetchSessionDetail(sessionId) {
  return fetchJson(`/sessions/${sessionId}`);
}

async function sendAction(action) {
  await fetchJson("/actions", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(action),
  });
}

async function putSettings(settings) {
  return fetchJson("/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(settings),
  });
}

async function purgeHistory() {
  return fetchJson("/sessions/purge", { method: "POST" });
}

async function deleteSession(sessionId) {
  return fetchJson(`/sessions/${sessionId}`, { method: "DELETE" });
}

async function exportSession(sessionId, format) {
  const response = await fetch(`${backendUrl}/sessions/${sessionId}/export?format=${format}`);
  if (!response.ok) {
    throw new Error(`export failed with ${response.status}`);
  }
  return response.text();
}

function setButtonVisualState(button, state, label = null) {
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

async function runWithButtonFeedback(button, task, labels = {}) {
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

function setChip(el, label, status) {
  el.textContent = label;
  el.classList.remove("ok", "warn", "error");
  if (status) {
    el.classList.add(status);
  }
}

function classifyState(stateValue) {
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

function renderSnapshot(snapshot) {
  state.snapshot = snapshot;

  setChip(els.backendChip, "Backend: connected", "ok");
  setChip(els.captureChip, `Capture: ${snapshot.capture_state}`, classifyState(snapshot.capture_state));
  setChip(els.cloudChip, `Cloud: ${snapshot.cloud_state}`, classifyState(snapshot.cloud_state));
  setChip(
    els.sttChip,
    `STT: ${snapshot.stt_provider || "unknown"}`,
    snapshot.cloud_state === "Error" ? "error" : "ok",
  );

  els.backendNote.textContent =
    snapshot.stt_status || "Backend connected. Waiting for the next state change.";
  els.partialBox.textContent = snapshot.transcript.partial_text || "No partial transcript yet.";
  els.currentSink.textContent = snapshot.current_sink || "unknown";
  els.monitorSource.textContent = snapshot.current_monitor_source || "unknown";
  els.sttStatus.textContent = snapshot.stt_status || "unknown";
  els.sessionId.textContent = snapshot.session_id || "not started";
  els.privacyPause.textContent = String(snapshot.privacy_pause);
  els.cloudPause.textContent = String(snapshot.cloud_pause);
  els.cloudAutoPause.textContent = String(snapshot.cloud_auto_pause);
  els.audioUploadActive.textContent = String(snapshot.audio_upload_active);
  els.privacyCapture.textContent = snapshot.privacy_pause
    ? "Local capture paused before transcript leaves the machine."
    : "System-audio monitor capture is active.";
  els.privacyCloud.textContent = snapshot.cloud_pause
    ? "Cloud processing is paused manually."
    : snapshot.cloud_auto_pause
      ? "Cloud upload is auto-paused because no recent speech was detected."
      : snapshot.audio_upload_active
        ? `Audio is currently uploading to ${snapshot.stt_provider || "the STT provider"}.`
        : `Connected to ${snapshot.stt_provider || "the STT provider"}, but silence is not being uploaded.`;
  els.privacyBackend.textContent = backendUrl;
  renderQuestionBanner(snapshot.detected_question);

  if (snapshot.transcript.segments.length === 0) {
    els.segmentList.innerHTML = `<div class="empty-state">No transcript segments committed yet.</div>`;
  } else {
    const detectedQuestionId = snapshot.detected_question?.id || null;
    const paragraphs = buildTranscriptParagraphs(snapshot.transcript.segments.slice(-40));
    els.segmentList.innerHTML = paragraphs
      .map(
        (paragraph) => `
          <p class="transcript-paragraph">
            ${paragraph
              .map((segment) => {
                const classes = ["transcript-fragment"];
                if (segment.id === detectedQuestionId) {
                  classes.push("transcript-question");
                }
                return `<span class="${classes.join(" ")}">${escapeHtml(segment.text)}</span>`;
              })
              .join(" ")}
          </p>`,
      )
      .join("");
  }

  if (snapshot.latest_assistant) {
    els.assistantCard.innerHTML = `
      <div class="assistant-meta">
        ${escapeHtml(snapshot.latest_assistant.kind)} • ${formatTime(snapshot.latest_assistant.created_at)}
      </div>
      <div class="assistant-content">${escapeHtml(snapshot.latest_assistant.content)}</div>
    `;
  } else {
    els.assistantCard.innerHTML = `
      <div class="assistant-meta">No assistant output yet.</div>
      <div class="assistant-content">Trigger an action once transcript is available.</div>
    `;
  }

  if (!snapshot.recent_errors.length) {
    els.errorList.innerHTML = `<div class="empty-state">No recent backend errors.</div>`;
  } else {
    els.errorList.innerHTML = snapshot.recent_errors
      .map((error) => `<div class="error-item">${escapeHtml(error)}</div>`)
      .join("");
  }
}

function buildTranscriptParagraphs(segments) {
  const paragraphs = [];
  let current = [];

  for (const segment of segments) {
    if (!current.length) {
      current.push(segment);
      continue;
    }

    const previous = current[current.length - 1];
    const gapMs = Math.max(0, segment.start_ms - previous.end_ms);
    const previousEndsSentence = /[.!?]["')\]]?$/.test(previous.text.trim());
    const shouldBreak =
      gapMs >= 1800 || (gapMs >= 900 && previousEndsSentence) || current.length >= 6;

    if (shouldBreak) {
      paragraphs.push(current);
      current = [segment];
    } else {
      current.push(segment);
    }
  }

  if (current.length) {
    paragraphs.push(current);
  }

  return paragraphs;
}

function renderQuestionBanner(question) {
  if (!question) {
    els.questionBanner.className = "question-banner question-banner-idle";
    els.questionBanner.innerHTML = `
      <div class="question-label">Question status</div>
      <div class="question-body">No question detected right now.</div>
    `;
    els.answerQuestionButton.textContent = "Answer Last Question";
    return;
  }

  els.questionBanner.className = "question-banner question-banner-detected";
  els.questionBanner.innerHTML = `
    <div class="question-label">Question detected</div>
    <div class="question-body">${escapeHtml(question.text)}</div>
    <div class="question-meta">${question.start_ms}-${question.end_ms} ms</div>
  `;
  els.answerQuestionButton.textContent = "Answer Detected Question";
}

function renderSettings(settings) {
  state.settings = settings;
  els.settingsMode.value = settings.default_mode;
  els.retentionDays.value = String(settings.retention_days);
  els.transcriptStorage.checked = settings.transcript_storage_enabled;
  els.autoStartCloud.checked = settings.auto_start_cloud;
  els.privacyStorage.textContent = settings.transcript_storage_enabled
    ? settings.retention_days === 0
      ? "Transcripts are stored locally with no automatic expiry."
      : `Transcripts are stored locally for ${settings.retention_days} days.`
    : "Transcript storage is disabled for new segments.";
  els.settingsNote.textContent =
    "Settings loaded from the local SQLite store. Automatic cloud resume is off by default.";
}

function renderSessions() {
  if (!state.sessions.length) {
    els.sessionList.innerHTML = `<div class="empty-state">No stored sessions yet.</div>`;
    return;
  }

  els.sessionList.innerHTML = state.sessions
    .map((session) => {
      const isActive = session.id === state.selectedSessionId;
      return `
        <button class="session-row ${isActive ? "active" : ""}" data-session-id="${session.id}">
          <div class="session-row-top">
            <span>${formatTime(session.started_at)}</span>
            <span>${escapeHtml(session.mode)}</span>
          </div>
          <div class="session-row-body">${escapeHtml(session.latest_transcript_excerpt || "No transcript excerpt yet.")}</div>
          <div class="session-row-meta">
            <span>${session.transcript_segment_count} transcript segments</span>
            <span>${session.assistant_event_count} assistant events</span>
          </div>
        </button>
      `;
    })
    .join("");

  document.querySelectorAll("[data-session-id]").forEach((button) => {
    button.addEventListener("click", async () => {
      const { sessionId } = button.dataset;
      if (!sessionId) {
        return;
      }
      try {
        await selectSession(sessionId);
      } catch (error) {
        renderDisconnected(error);
      }
    });
  });
}

function renderSessionDetail() {
  const session = state.selectedSession;
  if (!session) {
    els.sessionDetail.innerHTML = `<div class="empty-state">Select a session to inspect it.</div>`;
    return;
  }

  const transcriptMarkup = session.transcript_segments.length
    ? session.transcript_segments
        .slice(-24)
        .map(
          (segment) => `
            <article class="detail-row">
              <div class="segment-meta">${segment.start_ms}-${segment.end_ms} ms • ${escapeHtml(segment.source)}</div>
              <div class="segment-text">${escapeHtml(segment.text)}</div>
            </article>`,
        )
        .join("")
    : `<div class="empty-state">No transcript segments were stored for this session.</div>`;

  const assistantMarkup = session.assistant_events.length
    ? session.assistant_events
        .map(
          (event) => `
            <article class="detail-row">
              <div class="segment-meta">${escapeHtml(event.kind)} • ${formatTime(event.created_at)}</div>
              <div class="segment-text">${escapeHtml(event.content)}</div>
            </article>`,
        )
        .join("")
    : `<div class="empty-state">No assistant events were stored for this session.</div>`;

  els.sessionDetail.innerHTML = `
    <div class="detail-header">
      <div>
        <h3>${formatTime(session.session.started_at)}</h3>
        <p>${escapeHtml(session.session.capture_device)} • ${escapeHtml(session.session.mode)}</p>
      </div>
      <div class="detail-actions">
        <button id="export-json" class="ghost">Export JSON</button>
        <button id="export-markdown" class="ghost">Export Markdown</button>
        <button id="delete-session" class="secondary">Delete Session</button>
      </div>
    </div>
    <div class="detail-stats">
      <div><span>Transcript</span><strong>${session.session.transcript_segment_count}</strong></div>
      <div><span>Assistant</span><strong>${session.session.assistant_event_count}</strong></div>
      <div><span>Ended</span><strong>${session.session.ended_at ? formatTime(session.session.ended_at) : "Running"}</strong></div>
    </div>
    <div class="detail-columns">
      <div>
        <h4>Transcript</h4>
        <div class="detail-list">${transcriptMarkup}</div>
      </div>
      <div>
        <h4>Assistant Output</h4>
        <div class="detail-list">${assistantMarkup}</div>
      </div>
    </div>
  `;

  document.querySelector("#export-json")?.addEventListener("click", async () => {
    const button = document.querySelector("#export-json");
    try {
      await runWithButtonFeedback(
        button,
        () => handleExport(session.session.id, "json"),
        { pending: "Exporting...", success: "JSON Ready", error: "Export Failed" },
      );
    } catch (error) {
      renderDisconnected(error);
    }
  });
  document.querySelector("#export-markdown")?.addEventListener("click", async () => {
    const button = document.querySelector("#export-markdown");
    try {
      await runWithButtonFeedback(
        button,
        () => handleExport(session.session.id, "markdown"),
        { pending: "Exporting...", success: "Markdown Ready", error: "Export Failed" },
      );
    } catch (error) {
      renderDisconnected(error);
    }
  });
  document.querySelector("#delete-session")?.addEventListener("click", async () => {
    const button = document.querySelector("#delete-session");
    const confirmed = window.confirm(
      "Delete this stored session, including transcript, assistant output, and audit events?",
    );
    if (!confirmed) {
      return;
    }
    try {
      await runWithButtonFeedback(
        button,
        async () => {
          await deleteSession(session.session.id);
          state.selectedSessionId = null;
          state.selectedSession = null;
          await refreshHistory(true);
          els.settingsNote.textContent = "Session deleted.";
        },
        { pending: "Deleting...", success: "Deleted", error: "Delete Failed" },
      );
    } catch (error) {
      renderDisconnected(error);
    }
  });
}

function renderDisconnected(error) {
  setChip(els.backendChip, "Backend: disconnected", "error");
  setChip(els.captureChip, "Capture: unknown", "warn");
  setChip(els.cloudChip, "Cloud: unknown", "warn");
  setChip(els.sttChip, "STT: unknown", "warn");
  els.backendNote.textContent = `Cannot reach backend at ${backendUrl}. Start it with: cargo run -p app_backend (${error.message})`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function formatTime(value) {
  return new Date(value).toLocaleString();
}

function buildSettingsPayload() {
  return {
    retention_days: Math.max(0, Number.parseInt(els.retentionDays.value || "0", 10) || 0),
    transcript_storage_enabled: els.transcriptStorage.checked,
    auto_start_cloud: els.autoStartCloud.checked,
    default_mode: els.settingsMode.value,
  };
}

async function selectSession(sessionId) {
  state.selectedSessionId = sessionId;
  state.selectedSession = await fetchSessionDetail(sessionId);
  renderSessions();
  renderSessionDetail();
}

async function refreshHistory(force = false) {
  const now = Date.now();
  if (!force && now - state.lastHistoryRefreshAt < 6000) {
    return;
  }

  state.sessions = await fetchSessions();
  state.lastHistoryRefreshAt = now;
  if (!state.selectedSessionId && state.sessions.length) {
    state.selectedSessionId = state.sessions[0].id;
  }
  renderSessions();

  if (state.selectedSessionId) {
    const exists = state.sessions.some((session) => session.id === state.selectedSessionId);
    if (!exists) {
      state.selectedSessionId = state.sessions[0]?.id || null;
    }
  }

  if (state.selectedSessionId) {
    state.selectedSession = await fetchSessionDetail(state.selectedSessionId);
  } else {
    state.selectedSession = null;
  }
  renderSessionDetail();
}

async function refreshSettings(force = false) {
  const now = Date.now();
  if (!force && now - state.lastSettingsRefreshAt < 12000) {
    return;
  }

  const settings = await fetchSettings();
  state.lastSettingsRefreshAt = now;
  if (!state.settings || JSON.stringify(state.settings) !== JSON.stringify(settings)) {
    renderSettings(settings);
  }
}

async function handleExport(sessionId, format) {
  try {
    const payload = await exportSession(sessionId, format);
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
    renderDisconnected(error);
  }
}

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
    default:
      return { pending: "Working...", success: "Done", error: "Failed" };
  }
}

async function handleSettingsSave() {
  const previousSettings = state.settings;
  const nextSettings = buildSettingsPayload();
  try {
    const saved = await putSettings(nextSettings);
    state.lastSettingsRefreshAt = Date.now();
    renderSettings(saved);
    if (!previousSettings || previousSettings.default_mode !== saved.default_mode) {
      await sendAction({ SetMode: saved.default_mode });
    }
    if (!previousSettings || previousSettings.auto_start_cloud !== saved.auto_start_cloud) {
      await sendAction(saved.auto_start_cloud ? "ResumeCloud" : "PauseCloud");
    }
    const snapshot = await fetchHealth();
    renderSnapshot(snapshot);
    els.settingsNote.textContent = "Settings saved and applied.";
  } catch (error) {
    renderDisconnected(error);
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
    const result = await purgeHistory();
    await refreshHistory(true);
    els.settingsNote.textContent = `Purged ${result.deleted} stale sessions.`;
    return true;
  } catch (error) {
    renderDisconnected(error);
    throw error;
  }
}

document.querySelectorAll("[data-action]").forEach((button, index) => {
  if (index >= 2) {
    button.classList.add("secondary");
  }
  button.addEventListener("click", async () => {
    try {
      await runWithButtonFeedback(
        button,
        async () => {
          await sendAction(button.dataset.action);
          const snapshot = await fetchHealth();
          renderSnapshot(snapshot);
        },
        actionLabels(button.dataset.action),
      );
    } catch (error) {
      renderDisconnected(error);
    }
  });
});

els.saveSettings.addEventListener("click", async () => {
  try {
    await runWithButtonFeedback(
      els.saveSettings,
      () => handleSettingsSave(),
      { pending: "Saving...", success: "Saved", error: "Save Failed" },
    );
  } catch (error) {
    renderDisconnected(error);
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
    renderDisconnected(error);
  }
});

async function refreshLoop() {
  try {
    const snapshot = await fetchHealth();
    renderSnapshot(snapshot);
    await refreshSettings(false);
    await refreshHistory(false);
  } catch (error) {
    renderDisconnected(error);
  } finally {
    window.setTimeout(refreshLoop, 300);
  }
}

els.privacyBackend.textContent = backendUrl;
refreshLoop();
