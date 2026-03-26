// @ts-check

import { classifyState, setChip } from "../lib/ui.js";
import { escapeHtml } from "../lib/utils.js";
import { renderAssistantCards } from "./assistant.js";
import { renderSelectionState, renderTranscriptPanel } from "./transcript.js";

export function renderSnapshot(state, els, backendUrl, snapshot, options = {}) {
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

  renderTranscriptPanel(state, els, options.app);
  renderAssistantCards(state, els);
  renderErrors(els, snapshot.recent_errors || []);
  renderSelectionState(state, els);
}

function renderErrors(els, recentErrors) {
  const filtered = recentErrors.filter((error) => error.trim().length > 0);
  if (!filtered.length) {
    els.errorList.innerHTML = `<div class="empty-state">No recent backend errors.</div>`;
  } else {
    els.errorList.innerHTML = filtered
      .map((error) => `<div class="error-item">${escapeHtml(error)}</div>`)
      .join("");
  }
}

