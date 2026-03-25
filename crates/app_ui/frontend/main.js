const DEFAULT_BACKEND_URL = "http://127.0.0.1:8765";
const backendUrl = window.localStorage.getItem("soundmind.backendUrl") || DEFAULT_BACKEND_URL;
const APP_VERSION = window.SOUNDMIND_VERSION || "0.2.1";
const THEME_STORAGE_KEY = "soundmind.theme";
const RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY = "soundmind.restoredTranscriptSessionId";

const state = {
  snapshot: null,
  settings: null,
  transcriptSelection: null,
  stickyTranscriptSelection: null,
  primingDocuments: [],
  sessions: [],
  selectedSessionId: null,
  selectedSession: null,
  lastSettingsRefreshAt: 0,
  lastPrimingRefreshAt: 0,
  lastHistoryRefreshAt: 0,
  lastTranscriptRenderKey: null,
  lastTranscriptPartialText: null,
  lastManualQuestionSelectionKey: null,
  manualQuestionSelection: null,
  manualQuestionSelectionSessionId: null,
  restoredTranscriptSessionId: window.localStorage.getItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY),
};

const els = {
  backendChip: document.querySelector("#backend-chip"),
  captureChip: document.querySelector("#capture-chip"),
  cloudChip: document.querySelector("#cloud-chip"),
  sttChip: document.querySelector("#stt-chip"),
  backendNote: document.querySelector("#backend-note"),
  transcriptHint: document.querySelector("#transcript-hint"),
  transcriptReturnLive: document.querySelector("#transcript-return-live"),
  questionBanner: document.querySelector("#question-banner"),
  partialBox: document.querySelector("#partial-box"),
  segmentList: document.querySelector("#segment-list"),
  assistantCard: document.querySelector("#assistant-card"),
  actionAnswerButton: document.querySelector("#action-answer-button"),
  actionSummaryButton: document.querySelector("#action-summary-button"),
  actionCommentButton: document.querySelector("#action-comment-button"),
  clearSelectionButton: document.querySelector("#clear-selection-button"),
  clearAllButton: document.querySelector("#clear-all-button"),
  selectionStatus: document.querySelector("#selection-status"),
  appVersion: document.querySelector("#app-version"),
  themeToggle: document.querySelector("#theme-toggle"),
  errorList: document.querySelector("#error-list"),
  settingsMode: document.querySelector("#settings-mode"),
  openaiModel: document.querySelector("#openai-model"),
  retentionDays: document.querySelector("#retention-days"),
  transcriptStorage: document.querySelector("#transcript-storage"),
  autoStartCloud: document.querySelector("#auto-start-cloud"),
  assistantInstruction: document.querySelector("#assistant-instruction"),
  saveSettings: document.querySelector("#save-settings"),
  saveAgentConfig: document.querySelector("#save-agent-config"),
  purgeHistory: document.querySelector("#purge-history"),
  primingFileInput: document.querySelector("#priming-file-input"),
  uploadPrimingDocuments: document.querySelector("#upload-priming-documents"),
  primingDocumentList: document.querySelector("#priming-document-list"),
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

async function fetchPrimingDocuments() {
  return fetchJson("/priming-documents");
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

async function uploadPrimingDocument(document) {
  return fetchJson("/priming-documents", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(document),
  });
}

async function deletePrimingDocument(documentId) {
  return fetchJson(`/priming-documents/${documentId}`, { method: "DELETE" });
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
  renderTranscriptPanel();

  renderAssistantCard(snapshot);

  const recentErrors = snapshot.recent_errors.filter((error) => error.trim().length > 0);
  if (!recentErrors.length) {
    els.errorList.innerHTML = `<div class="empty-state">No recent backend errors.</div>`;
  } else {
    els.errorList.innerHTML = recentErrors
      .map((error) => `<div class="error-item">${escapeHtml(error)}</div>`)
      .join("");
  }

  renderSelectionState();
}

function renderAssistantContent(content, kind = "Notice") {
  const lines = content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);

  if (!lines.length) {
    return `<p>No assistant output yet.</p>`;
  }

  const bulletLines = lines.filter((line) => /^[*\-•]\s+/.test(line));
  if (bulletLines.length >= 1 && bulletLines.length === lines.length) {
    return `
      <ul class="assistant-bullets">
        ${bulletLines
          .map((line) => `<li>${escapeHtml(line.replace(/^[*\-•]\s+/, ""))}</li>`)
          .join("")}
      </ul>
    `;
  }

  const shouldForceBullets = kind !== "Notice";
  if (shouldForceBullets) {
    const bulletCandidates = contentToBulletCandidates(lines);
    if (bulletCandidates.length >= 2) {
      return `
        <ul class="assistant-bullets">
          ${bulletCandidates.map((line) => `<li>${escapeHtml(line)}</li>`).join("")}
        </ul>
      `;
    }
  }

  return lines.map((line) => `<p>${escapeHtml(line)}</p>`).join("");
}

function renderAssistantCard(snapshot) {
  const latestAssistant = snapshot.latest_assistant;
  if (!latestAssistant) {
    els.assistantCard.innerHTML = `
      <div class="assistant-meta">No assistant output yet.</div>
      <div class="assistant-content">Trigger an action once transcript is available.</div>
    `;
    return;
  }

  const questionContext = currentQuestionContextForAssistant(latestAssistant.kind);
  const questionMarkup = questionContext
    ? `
      <div class="assistant-question">
        <div class="assistant-question-label">Question</div>
        <div class="assistant-question-text">${escapeHtml(questionContext)}</div>
      </div>
    `
    : "";

  els.assistantCard.innerHTML = `
    <div class="assistant-meta">
      ${escapeHtml(latestAssistant.kind)} • ${formatTime(latestAssistant.created_at)}
    </div>
    ${questionMarkup}
    <div class="assistant-content">${renderAssistantContent(latestAssistant.content, latestAssistant.kind)}</div>
  `;
}

function currentQuestionContextForAssistant(kind) {
  if (kind !== "Answer") {
    return null;
  }

  const selection = currentManualQuestionSelection() || state.transcriptSelection || state.stickyTranscriptSelection;
  if (selection && selection.selected_text.trim()) {
    return selection.selected_text.trim();
  }

  const detectedQuestion = currentDetectedQuestion();
  return detectedQuestion?.text?.trim() || null;
}

function contentToBulletCandidates(lines) {
  const merged = lines.join(" ");
  return merged
    .split(/(?<=[.!?])\s+(?=[A-Z0-9])/)
    .map((line) => line.trim().replace(/^[*\-•]\s+/, ""))
    .filter((line) => line.length > 0);
}

function bindTranscriptInteractions() {
  document.querySelectorAll("[data-question-segment-id]").forEach((button) => {
    button.addEventListener("click", async () => {
      const segmentId = button.dataset.questionSegmentId;
      if (!segmentId) {
        return;
      }

      if (isRestoredTranscriptView()) {
        const segment = currentTranscriptSegments().find(
          (candidate) => String(candidate.id) === String(segmentId),
        );
        if (!segment) {
          return;
        }
        state.transcriptSelection = { selected_text: segment.text, segment_ids: [segment.id] };
        state.stickyTranscriptSelection = state.transcriptSelection;
        renderSelectionState();
        return;
      }

      try {
        await runWithButtonFeedback(
          button,
          async () => {
            await sendAction({ AnswerQuestionBySegment: { segment_id: segmentId } });
            const snapshot = await fetchHealth();
            renderSnapshot(snapshot);
          },
          { pending: "Answering...", success: "Answer Ready", error: "Answer Failed" },
        );
      } catch (error) {
        renderDisconnected(error);
      }
    });
  });
}

function updateTranscriptSelection() {
  const nextSelection = snapshotTranscriptSelection();
  if (!nextSelection) {
    state.transcriptSelection = null;
    renderSelectionState();
    return;
  }

  state.transcriptSelection = nextSelection;
  state.stickyTranscriptSelection = nextSelection;
  clearManualQuestionSelection();
  renderSelectionState();
}

function snapshotTranscriptSelection() {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
    return null;
  }

  const range = selection.getRangeAt(0);
  if (!selectionIsInsideTranscript(range)) {
    return null;
  }

  const selectedText = selection.toString().trim();
  if (!selectedText) {
    return null;
  }

  const segmentIds = Array.from(document.querySelectorAll("[data-segment-id]"))
    .filter((element) => range.intersectsNode(element))
    .map((element) => element.dataset.segmentId)
    .filter((segmentId) => Boolean(segmentId));

  return { selected_text: selectedText, segment_ids: [...new Set(segmentIds)] };
}

function selectionIsInsideTranscript(range) {
  const container = els.segmentList;
  if (!container) {
    return false;
  }

  const commonAncestor = range.commonAncestorContainer;
  return commonAncestor instanceof Element
    ? container.contains(commonAncestor)
    : container.contains(commonAncestor.parentElement);
}

function renderSelectionState() {
  const selection = state.transcriptSelection || state.stickyTranscriptSelection;
  const restoredTranscript = isRestoredTranscriptView();

  els.actionAnswerButton.disabled = restoredTranscript;
  els.actionSummaryButton.disabled = restoredTranscript;
  els.actionCommentButton.disabled = restoredTranscript;

  if (restoredTranscript) {
    els.actionAnswerButton.textContent = "Answer Last Question";
    els.actionSummaryButton.textContent = "Summarize Recent";
    els.actionCommentButton.textContent = "Comment on Topic";
    els.selectionStatus.className = selection ? "selection-status active" : "selection-status";
    els.selectionStatus.textContent = selection
      ? "Selection active in restored session. Return to live transcript to run actions."
      : "Viewing a stored session in the transcript panel. Return to live transcript to run actions.";
    els.clearSelectionButton.disabled = !selection;
    return;
  }

  if (!selection) {
    els.selectionStatus.className = "selection-status";
    els.selectionStatus.textContent =
      "No transcript selection. Actions use the latest detected question or recent transcript.";
    els.clearSelectionButton.disabled = true;
    els.actionAnswerButton.disabled = false;
    els.actionSummaryButton.disabled = false;
    els.actionCommentButton.disabled = false;
    els.actionAnswerButton.textContent =
      state.snapshot?.detected_question ? "Answer Detected Question" : "Answer Last Question";
    els.actionSummaryButton.textContent = "Summarize Recent";
    els.actionCommentButton.textContent = "Comment on Topic";
    return;
  }

  els.selectionStatus.className = "selection-status active";
  els.selectionStatus.textContent = selection.segment_ids.length
    ? `Selection active across ${selection.segment_ids.length} segment${selection.segment_ids.length === 1 ? "" : "s"}. Top actions now target the selected excerpt.`
    : "Selection active in the live transcript text. Top actions now target the selected excerpt.";
  els.clearSelectionButton.disabled = false;
  els.actionAnswerButton.disabled = false;
  els.actionSummaryButton.disabled = false;
  els.actionCommentButton.disabled = false;
  els.actionAnswerButton.textContent = "Answer Selection";
  els.actionSummaryButton.textContent = "Summarize Selection";
  els.actionCommentButton.textContent = "Comment on Selection";
}

function clearTranscriptSelection() {
  const selection = window.getSelection();
  if (selection) {
    selection.removeAllRanges();
  }
  state.transcriptSelection = null;
  state.stickyTranscriptSelection = null;
  renderSelectionState();
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

function isRestoredTranscriptView() {
  return Boolean(
    state.restoredTranscriptSessionId &&
      state.selectedSession &&
      state.selectedSession.session.id === state.restoredTranscriptSessionId,
  );
}

function currentTranscriptSegments() {
  if (isRestoredTranscriptView()) {
    return state.selectedSession?.transcript_segments || [];
  }
  return state.snapshot?.transcript?.segments || [];
}

function currentDetectedQuestion() {
  if (isRestoredTranscriptView()) {
    return [...currentTranscriptSegments()].reverse().find((segment) => segment.is_question_candidate) || null;
  }
  return state.snapshot?.detected_question || null;
}

function renderTranscriptPanel() {
  const transcriptScrollState = captureTranscriptScrollState();
  const restoredTranscript = isRestoredTranscriptView();
  const segments = currentTranscriptSegments();
  const partialText = restoredTranscript ? "" : normalizeTranscriptText(state.snapshot?.transcript?.partial_text || "");
  const manualSelection = currentManualQuestionSelection();
  const manualSelectionKey = manualSelection
    ? `${state.manualQuestionSelectionSessionId || ""}:${manualSelection.segment_ids.join(",")}:${manualSelection.selected_text}`
    : "";
  const transcriptRenderKey = buildTranscriptRenderKey(segments, restoredTranscript, manualSelectionKey);
  const questionButtonTitle = restoredTranscript ? "Select this question" : "Answer this question";

  els.transcriptHint.textContent = restoredTranscript
    ? "Viewing a stored session transcript. Scroll through it, click `?` to select a detected question, or return to the live transcript."
    : "Scroll continuously, click `?` beside detected questions, or select text and use the action bar above.";
  els.transcriptReturnLive.hidden = !restoredTranscript;
  els.partialBox.textContent = restoredTranscript
    ? `Stored session restored. ${segments.length} committed transcript segment${segments.length === 1 ? "" : "s"} available.`
    : partialText || "No partial transcript yet.";

  renderQuestionBanner(currentDetectedQuestion(), restoredTranscript);
  renderSelectionState();

  const transcriptChanged = state.lastTranscriptRenderKey !== transcriptRenderKey;
  const partialChanged = state.lastTranscriptPartialText !== partialText;
  const partialPresenceChanged = Boolean(state.lastTranscriptPartialText) !== Boolean(partialText);
  const manualSelectionChanged = state.lastManualQuestionSelectionKey !== manualSelectionKey;

  if (!transcriptChanged && !partialChanged && !manualSelectionChanged) {
    return;
  }

  if (transcriptChanged || partialPresenceChanged) {
    const transcriptMarkup = renderTranscriptMarkup({
      segments,
      partialText,
      restoredTranscript,
      manualSelection,
      questionButtonTitle,
    });

    els.segmentList.innerHTML = transcriptMarkup;
    bindTranscriptInteractions();
    state.lastTranscriptRenderKey = transcriptRenderKey;
  } else {
    updateLivePartialText(partialText);
  }

  state.lastTranscriptPartialText = partialText;
  state.lastManualQuestionSelectionKey = manualSelectionKey;
  restoreTranscriptScrollState(transcriptScrollState);
}

function buildTranscriptRenderKey(segments, restoredTranscript, manualSelectionKey) {
  const sessionKey = restoredTranscript
    ? state.selectedSession?.session?.id || "restored"
    : state.snapshot?.session_id || "live";

  return [
    restoredTranscript ? "restored" : "live",
    sessionKey,
    manualSelectionKey || "none",
    ...segments.map((segment) => `${segment.id}:${segment.start_ms}:${segment.end_ms}:${segment.text}`),
  ].join("|");
}

function renderTranscriptMarkup({ segments, partialText, restoredTranscript, manualSelection, questionButtonTitle }) {
  if (segments.length === 0 && !partialText) {
    return `<div class="empty-state">${
      restoredTranscript ? "No transcript segments were stored for this session." : "No transcript segments committed yet."
    }</div>`;
  }

  const committedMarkup = segments.length
    ? buildTranscriptParagraphs(segments)
        .map(
          (paragraph) => `
            <p class="transcript-paragraph">
              ${renderTranscriptParagraph(paragraph, manualSelection, questionButtonTitle)}
            </p>`,
        )
        .join("")
    : "";

  const partialMarkup = partialText
    ? `
        <div class="transcript-live-partial" aria-live="polite">
          <span class="transcript-live-label">Live</span>
          <span class="transcript-live-text" id="transcript-live-partial-text">${escapeHtml(partialText)}</span>
        </div>
      `
    : "";

  return `${committedMarkup}${partialMarkup}`;
}

function renderTranscriptParagraph(paragraph, manualSelection, questionButtonTitle) {
  const manualIds = new Set((manualSelection?.segment_ids || []).map((segmentId) => String(segmentId)));
  const manualAnchorId = manualSelection?.segment_ids?.length
    ? String(manualSelection.segment_ids[manualSelection.segment_ids.length - 1])
    : null;
  const parts = [];

  for (let index = 0; index < paragraph.length; index += 1) {
    const segment = paragraph[index];
    const segmentId = String(segment.id);

    if (manualIds.has(segmentId)) {
      const run = [segment];
      while (index + 1 < paragraph.length && manualIds.has(String(paragraph[index + 1].id))) {
        index += 1;
        run.push(paragraph[index]);
      }
      parts.push(renderManualTranscriptRun(run, manualAnchorId, questionButtonTitle));
      continue;
    }

    parts.push(renderStandardTranscriptSegment(segment, questionButtonTitle));
  }

  return parts.join(" ");
}

function renderManualTranscriptRun(run, manualAnchorId, questionButtonTitle) {
  const anchorSegment = run.find((segment) => String(segment.id) === String(manualAnchorId)) || run[run.length - 1];
  const questionButton = anchorSegment
    ? `<button class="question-inline-button secondary" data-question-segment-id="${anchorSegment.id}" title="${questionButtonTitle}">?</button>`
    : "";
  const content = run
    .map(
      (segment) =>
        `<span class="transcript-fragment-piece" data-segment-id="${segment.id}">${escapeHtml(segment.text)}</span>`,
    )
    .join(" ");

  return `
    <span class="transcript-fragment-line transcript-fragment-line-manual">
      <span class="transcript-fragment transcript-question transcript-question-manual transcript-manual-run">${content}</span>
      ${questionButton}
    </span>
  `;
}

function renderStandardTranscriptSegment(segment, questionButtonTitle) {
  const classes = ["transcript-fragment"];
  if (segment.is_question_candidate) {
    classes.push("transcript-question");
  }
  const questionButton = segment.is_question_candidate
    ? `<button class="question-inline-button secondary" data-question-segment-id="${segment.id}" title="${questionButtonTitle}">?</button>`
    : "";

  return `
    <span class="transcript-fragment-line">
      <span class="${classes.join(" ")}" data-segment-id="${segment.id}">${escapeHtml(segment.text)}</span>
      ${questionButton}
    </span>
  `;
}

function updateLivePartialText(partialText) {
  const liveText = document.querySelector("#transcript-live-partial-text");
  if (!liveText) {
    return;
  }

  liveText.textContent = partialText;
}

function normalizeTranscriptText(text) {
  return String(text || "").trim();
}

function currentManualQuestionSelection() {
  if (!state.manualQuestionSelection || !state.manualQuestionSelectionSessionId) {
    return null;
  }

  const currentSessionId = currentTranscriptSessionId();
  if (!currentSessionId || String(currentSessionId) !== String(state.manualQuestionSelectionSessionId)) {
    return null;
  }

  return state.manualQuestionSelection;
}

function currentTranscriptSessionId() {
  return isRestoredTranscriptView()
    ? state.selectedSession?.session?.id || null
    : state.snapshot?.session_id || null;
}

function promoteManualQuestionSelection(selection) {
  if (!selection || !Array.isArray(selection.segment_ids) || !selection.segment_ids.length) {
    return;
  }

  state.manualQuestionSelection = {
    selected_text: String(selection.selected_text || "").trim(),
    segment_ids: [...new Set(selection.segment_ids)],
  };
  state.manualQuestionSelectionSessionId = currentTranscriptSessionId();
  state.lastManualQuestionSelectionKey = null;
}

function clearManualQuestionSelection() {
  state.manualQuestionSelection = null;
  state.manualQuestionSelectionSessionId = null;
  state.lastManualQuestionSelectionKey = null;
}

function captureTranscriptScrollState() {
  const container = els.segmentList;
  if (!container) {
    return null;
  }

  const distanceFromBottom = container.scrollHeight - container.scrollTop - container.clientHeight;
  return {
    scrollTop: container.scrollTop,
    wasNearBottom: distanceFromBottom <= 24,
  };
}

function restoreTranscriptScrollState(previousState) {
  const container = els.segmentList;
  if (!container || !previousState) {
    return;
  }

  if (previousState.wasNearBottom) {
    container.scrollTop = container.scrollHeight;
    return;
  }

  const maxScrollTop = Math.max(0, container.scrollHeight - container.clientHeight);
  container.scrollTop = Math.min(previousState.scrollTop, maxScrollTop);
}

function renderQuestionBanner(question, restoredTranscript = false) {
  if (restoredTranscript && !question) {
    els.questionBanner.className = "question-banner question-banner-idle";
    els.questionBanner.innerHTML = `
      <div class="question-label">Stored session</div>
      <div class="question-body">No detected question marker was stored for this session.</div>
    `;
    return;
  }

  if (restoredTranscript && question) {
    els.questionBanner.className = "question-banner question-banner-detected";
    els.questionBanner.innerHTML = `
      <div class="question-label">Stored question</div>
      <div class="question-body">${escapeHtml(question.text)}</div>
      <div class="question-meta">${question.start_ms}-${question.end_ms} ms</div>
    `;
    return;
  }

  if (!question) {
    els.questionBanner.className = "question-banner question-banner-idle";
    els.questionBanner.innerHTML = `
      <div class="question-label">Question status</div>
      <div class="question-body">No question detected right now.</div>
    `;
    if (!state.transcriptSelection) {
      els.actionAnswerButton.textContent = "Answer Last Question";
    }
    return;
  }

  els.questionBanner.className = "question-banner question-banner-detected";
  els.questionBanner.innerHTML = `
    <div class="question-label">Question detected</div>
    <div class="question-body">${escapeHtml(question.text)}</div>
    <div class="question-meta">${question.start_ms}-${question.end_ms} ms</div>
  `;
  if (!state.transcriptSelection) {
    els.actionAnswerButton.textContent = "Answer Detected Question";
  }
}

function renderSettings(settings) {
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

function renderPrimingDocuments() {
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
            await deletePrimingDocument(documentDelete);
            await refreshPrimingDocuments(true);
            els.settingsNote.textContent = "Priming document deleted.";
          },
          { pending: "Deleting...", success: "Deleted", error: "Delete Failed" },
        );
      } catch (error) {
        renderDisconnected(error);
      }
    });
  });
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
        <button id="restore-transcript" class="ghost">Restore in Transcript</button>
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

  document.querySelector("#restore-transcript")?.addEventListener("click", () => {
    state.restoredTranscriptSessionId = session.session.id;
    clearTranscriptSelection();
    renderTranscriptPanel();
    els.segmentList.scrollTop = 0;
  });
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
          state.restoredTranscriptSessionId = null;
          window.localStorage.removeItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY);
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

function preferredTheme() {
  const storedTheme = window.localStorage.getItem(THEME_STORAGE_KEY);
  if (storedTheme === "light" || storedTheme === "dark") {
    return storedTheme;
  }

  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(theme) {
  const resolvedTheme = theme === "dark" ? "dark" : "light";
  document.documentElement.dataset.theme = resolvedTheme;
  if (els.themeToggle) {
    els.themeToggle.checked = resolvedTheme === "dark";
  }
  window.localStorage.setItem(THEME_STORAGE_KEY, resolvedTheme);
}

function buildSettingsPayload() {
  return {
    retention_days: Math.max(0, Number.parseInt(els.retentionDays.value || "0", 10) || 0),
    transcript_storage_enabled: els.transcriptStorage.checked,
    auto_start_cloud: els.autoStartCloud.checked,
    default_mode: els.settingsMode.value,
    openai_model: els.openaiModel.value.trim() || "gpt-4o-mini",
    assistant_instruction: els.assistantInstruction.value.trim(),
  };
}

async function selectSession(sessionId) {
  state.selectedSessionId = sessionId;
  state.selectedSession = await fetchSessionDetail(sessionId);
  state.restoredTranscriptSessionId = sessionId;
  window.localStorage.setItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY, sessionId);
  renderSessions();
  renderSessionDetail();
  renderTranscriptPanel();
}

async function refreshHistory(force = false) {
  const now = Date.now();
  if (!force && now - state.lastHistoryRefreshAt < 6000) {
    return;
  }

  state.sessions = await fetchSessions();
  state.lastHistoryRefreshAt = now;
  const restoredExists = state.restoredTranscriptSessionId
    ? state.sessions.some((session) => session.id === state.restoredTranscriptSessionId)
    : false;

  if (state.restoredTranscriptSessionId && restoredExists) {
    state.selectedSessionId = state.restoredTranscriptSessionId;
  } else if (!state.selectedSessionId && state.sessions.length) {
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
  renderTranscriptPanel();
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

async function refreshPrimingDocuments(force = false) {
  const now = Date.now();
  if (!force && now - state.lastPrimingRefreshAt < 12000) {
    return;
  }

  state.primingDocuments = await fetchPrimingDocuments();
  state.lastPrimingRefreshAt = now;
  renderPrimingDocuments();
}

async function fileToUploadPayload(file) {
  const buffer = await file.arrayBuffer();
  return {
    file_name: file.name,
    mime_type: file.type || null,
    content_base64: arrayBufferToBase64(buffer),
  };
}

function arrayBufferToBase64(buffer) {
  const bytes = new Uint8Array(buffer);
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return window.btoa(binary);
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

function resolvePrimaryAction(action) {
  const selection = state.transcriptSelection || state.stickyTranscriptSelection;
  if (!selection) {
    return action;
  }

  switch (action) {
    case "AnswerLastQuestion":
      return { AnswerSelection: selection };
    case "SummariseLastMinute":
      return { SummariseSelection: selection };
    case "CommentCurrentTopic":
      return { CommentSelection: selection };
    default:
      return action;
  }
}

function actionKey(action) {
  return typeof action === "string" ? action : Object.keys(action)[0];
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
    if (!previousSettings || previousSettings.openai_model !== saved.openai_model) {
      els.settingsNote.textContent = `OpenAI model switched to ${saved.openai_model}.`;
    }
    const snapshot = await fetchHealth();
    renderSnapshot(snapshot);
    if (!previousSettings || previousSettings.openai_model === saved.openai_model) {
      els.settingsNote.textContent = "Settings saved and applied.";
    }
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

async function handlePrimingUpload() {
  const files = Array.from(els.primingFileInput.files || []);
  if (!files.length) {
    els.settingsNote.textContent = "Choose one or more documents to upload first.";
    return false;
  }

  for (const file of files) {
    const payload = await fileToUploadPayload(file);
    await uploadPrimingDocument(payload);
  }

  els.primingFileInput.value = "";
  await refreshPrimingDocuments(true);
  els.settingsNote.textContent = `Uploaded ${files.length} priming document${files.length === 1 ? "" : "s"}.`;
  return true;
}

async function handleClearAll() {
  clearTranscriptSelection();
  clearManualQuestionSelection();
  await sendAction("ClearCurrentView");
  const snapshot = await fetchHealth();
  renderSnapshot(snapshot);
  return true;
}

document.querySelectorAll(".hero-actions [data-action]").forEach((button) => {
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

[
  [els.actionAnswerButton, "AnswerLastQuestion"],
  [els.actionSummaryButton, "SummariseLastMinute"],
  [els.actionCommentButton, "CommentCurrentTopic"],
].forEach(([button, defaultAction]) => {
  button.addEventListener("mousedown", (event) => {
    const selection = snapshotTranscriptSelection();
    if (selection) {
      state.transcriptSelection = selection;
      state.stickyTranscriptSelection = selection;
      clearManualQuestionSelection();
      renderSelectionState();
    }
    event.preventDefault();
  });
  button.addEventListener("click", async () => {
    const action = resolvePrimaryAction(defaultAction);
    try {
      await runWithButtonFeedback(
        button,
        async () => {
          await sendAction(action);
          const snapshot = await fetchHealth();
          renderSnapshot(snapshot);
          if (actionKey(action) === "AnswerSelection") {
            promoteManualQuestionSelection(action.AnswerSelection);
            renderTranscriptPanel();
          }
        },
        actionLabels(actionKey(action)),
      );
    } catch (error) {
      renderDisconnected(error);
    }
  });
});

els.clearSelectionButton.addEventListener("mousedown", (event) => {
  event.preventDefault();
});
els.clearSelectionButton.addEventListener("click", () => {
  clearTranscriptSelection();
});

els.transcriptReturnLive?.addEventListener("click", () => {
  state.restoredTranscriptSessionId = null;
  window.localStorage.removeItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY);
  clearTranscriptSelection();
  clearManualQuestionSelection();
  renderTranscriptPanel();
});

els.clearAllButton.addEventListener("mousedown", (event) => {
  event.preventDefault();
});
els.clearAllButton.addEventListener("click", async () => {
  try {
    await runWithButtonFeedback(
      els.clearAllButton,
      () => handleClearAll(),
      { pending: "Clearing...", success: "Cleared", error: "Clear Failed" },
    );
  } catch (error) {
    renderDisconnected(error);
  }
});

document.addEventListener("selectionchange", () => {
  updateTranscriptSelection();
});

els.appVersion.textContent = `v${APP_VERSION}`;
applyTheme(preferredTheme());
els.themeToggle?.addEventListener("change", () => {
  applyTheme(els.themeToggle.checked ? "dark" : "light");
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

els.openaiModel.addEventListener("change", async () => {
  try {
    await handleSettingsSave();
  } catch (error) {
    renderDisconnected(error);
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

els.uploadPrimingDocuments.addEventListener("click", async () => {
  try {
    await runWithButtonFeedback(
      els.uploadPrimingDocuments,
      () => handlePrimingUpload(),
      { pending: "Uploading...", success: "Uploaded", error: "Upload Failed" },
    );
  } catch (error) {
    renderDisconnected(error);
    els.settingsNote.textContent =
      "Document upload failed. Text files work directly; PDF upload requires `pdftotext`.";
  }
});

async function refreshLoop() {
  try {
    const snapshot = await fetchHealth();
    renderSnapshot(snapshot);
    await refreshSettings(false);
    await refreshPrimingDocuments(false);
    await refreshHistory(false);
  } catch (error) {
    renderDisconnected(error);
  } finally {
    window.setTimeout(refreshLoop, 300);
  }
}

els.privacyBackend.textContent = backendUrl;
refreshLoop();
