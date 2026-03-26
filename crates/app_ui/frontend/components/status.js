// @ts-check

import { setStatusBlock } from "../lib/ui.js";
import { escapeHtml } from "../lib/utils.js";
import { renderAssistantCards } from "./assistant.js";
import { renderSelectionState, renderTranscriptPanel } from "./transcript.js";

export function renderSnapshot(state, els, backendUrl, snapshot, options = {}) {
  state.snapshot = snapshot;

  const backendStatus = describeBackend(snapshot);
  const captureStatus = describeCapture(snapshot);
  const cloudStatus = describeCloud(snapshot);

  setStatusBlock(
    els.backendBlock,
    els.backendChip,
    els.backendDetail,
    backendStatus.value,
    backendStatus.detail,
    backendStatus.status,
  );
  setStatusBlock(
    els.captureBlock,
    els.captureChip,
    els.captureDetail,
    captureStatus.value,
    captureStatus.detail,
    captureStatus.status,
  );
  setStatusBlock(
    els.cloudBlock,
    els.cloudChip,
    els.cloudDetail,
    cloudStatus.value,
    cloudStatus.detail,
    cloudStatus.status,
  );
  if (els.captureIndicator) {
    els.captureIndicator.dataset.state = captureStatus.indicator;
  }

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

function describeBackend(snapshot) {
  const provider = snapshot.stt_provider || "STT";
  if (snapshot.cloud_state === "Error") {
    return {
      value: "Backend connected",
      detail: `${provider} reported an error. Check the recent error list below.`,
      status: "warn",
    };
  }

  return {
    value: "Backend connected",
    detail: snapshot.stt_status || `Listening on the local backend and ready to process audio.`,
    status: "ok",
  };
}

function describeCapture(snapshot) {
  if (snapshot.privacy_pause || snapshot.capture_state === "Paused") {
    return {
      value: "Capture paused",
      detail: "Local system-audio capture is paused before transcript leaves the machine.",
      status: "warn",
      indicator: "paused",
    };
  }

  if (snapshot.capture_state === "Capturing") {
    return {
      value: "Capturing system audio",
      detail: snapshot.current_monitor_source
        ? `Listening to ${snapshot.current_monitor_source}.`
        : "Listening to the current system-audio monitor source.",
      status: "ok",
      indicator: "active",
    };
  }

  if (snapshot.capture_state === "Error") {
    return {
      value: "Capture unavailable",
      detail: "The capture pipeline reported an error. Review backend status and errors below.",
      status: "error",
      indicator: "error",
    };
  }

  return {
    value: "Not capturing",
    detail: "Soundmind is connected, but local system-audio capture is inactive.",
    status: null,
    indicator: "idle",
  };
}

function describeCloud(snapshot) {
  const provider = snapshot.stt_provider || "the STT provider";

  if (snapshot.cloud_state === "Error") {
    return {
      value: "Cloud unavailable",
      detail: snapshot.stt_status || `The connection to ${provider} is in an error state.`,
      status: "error",
    };
  }

  if (snapshot.cloud_pause) {
    return {
      value: "Cloud paused",
      detail: "Cloud processing is paused manually.",
      status: "warn",
    };
  }

  if (snapshot.cloud_auto_pause) {
    return {
      value: "Cloud auto-paused",
      detail: "Speech has been idle long enough that audio upload is temporarily paused.",
      status: "warn",
    };
  }

  if (snapshot.audio_upload_active) {
    return {
      value: "Uploading audio",
      detail: `Detected speech is currently uploading to ${provider}.`,
      status: "ok",
    };
  }

  if (snapshot.capture_state === "Capturing") {
    return {
      value: "Connected, waiting for speech",
      detail: `Local capture is active, but silence is not being uploaded to ${provider}.`,
      status: null,
    };
  }

  return {
    value: "Cloud idle",
    detail: `Cloud processing is available, but local capture is not active.`,
    status: null,
  };
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
