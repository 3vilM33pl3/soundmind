const DEFAULT_BACKEND_URL = "http://127.0.0.1:8765";
const backendUrl = window.localStorage.getItem("soundmind.backendUrl") || DEFAULT_BACKEND_URL;

const els = {
  backendChip: document.querySelector("#backend-chip"),
  captureChip: document.querySelector("#capture-chip"),
  cloudChip: document.querySelector("#cloud-chip"),
  sttChip: document.querySelector("#stt-chip"),
  backendNote: document.querySelector("#backend-note"),
  modeSelect: document.querySelector("#mode-select"),
  currentSink: document.querySelector("#current-sink"),
  monitorSource: document.querySelector("#monitor-source"),
  sessionId: document.querySelector("#session-id"),
  privacyPause: document.querySelector("#privacy-pause"),
  cloudPause: document.querySelector("#cloud-pause"),
  partialBox: document.querySelector("#partial-box"),
  segmentList: document.querySelector("#segment-list"),
  assistantCard: document.querySelector("#assistant-card"),
  errorList: document.querySelector("#error-list"),
  applyMode: document.querySelector("#apply-mode"),
};

async function fetchHealth() {
  const response = await fetch(`${backendUrl}/health`);
  if (!response.ok) {
    throw new Error(`Backend returned ${response.status}`);
  }
  return response.json();
}

async function sendAction(action) {
  const response = await fetch(`${backendUrl}/actions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(action),
  });

  if (!response.ok) {
    throw new Error(`Action failed with ${response.status}`);
  }
}

function setChip(el, label, status) {
  el.textContent = label;
  el.classList.remove("ok", "warn", "error");
  if (status) {
    el.classList.add(status);
  }
}

function render(snapshot) {
  setChip(els.backendChip, "Backend: connected", "ok");
  setChip(els.captureChip, `Capture: ${snapshot.capture_state}`, classifyState(snapshot.capture_state));
  setChip(els.cloudChip, `Cloud: ${snapshot.cloud_state}`, classifyState(snapshot.cloud_state));
  setChip(
    els.sttChip,
    `STT: ${snapshot.stt_provider || "unknown"}`,
    snapshot.stt_status && snapshot.cloud_state === "Error" ? "error" : "ok",
  );

  els.backendNote.textContent = snapshot.stt_status || "Backend connected. Waiting for the next state change.";
  els.modeSelect.value = snapshot.mode;
  els.currentSink.textContent = snapshot.current_sink || "unknown";
  els.monitorSource.textContent = snapshot.current_monitor_source || "unknown";
  els.sessionId.textContent = snapshot.session_id || "not started";
  els.privacyPause.textContent = String(snapshot.privacy_pause);
  els.cloudPause.textContent = String(snapshot.cloud_pause);
  els.partialBox.textContent = snapshot.transcript.partial_text || "No partial transcript yet.";

  if (snapshot.transcript.segments.length === 0) {
    els.segmentList.innerHTML = `<div class="empty-state">No transcript segments committed yet.</div>`;
  } else {
    els.segmentList.innerHTML = snapshot.transcript.segments
      .slice(-12)
      .map(
        (segment) => `
          <article class="segment">
            <div class="segment-meta">${segment.source} • ${segment.start_ms}-${segment.end_ms} ms</div>
            <div class="segment-text">${escapeHtml(segment.text)}</div>
          </article>`,
      )
      .join("");
  }

  if (snapshot.latest_assistant) {
    els.assistantCard.innerHTML = `
      <div class="assistant-meta">
        ${snapshot.latest_assistant.kind} • ${new Date(snapshot.latest_assistant.created_at).toLocaleString()}
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

function renderDisconnected(error) {
  setChip(els.backendChip, "Backend: disconnected", "error");
  setChip(els.captureChip, "Capture: unknown", "warn");
  setChip(els.cloudChip, "Cloud: unknown", "warn");
  setChip(els.sttChip, "STT: unknown", "warn");
  els.backendNote.textContent = `Cannot reach backend at ${backendUrl}. Start it with: cargo run -p app_backend (${error.message})`;
}

function classifyState(state) {
  if (state === "Capturing" || state === "SttActive" || state === "LlmActive") return "ok";
  if (state === "Paused") return "warn";
  if (state === "Error") return "error";
  return null;
}

function escapeHtml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

document.querySelectorAll("[data-action]").forEach((button, index) => {
  if (index >= 2) {
    button.classList.add("secondary");
  }
  button.addEventListener("click", async () => {
    try {
      await sendAction(button.dataset.action);
      const snapshot = await fetchHealth();
      render(snapshot);
    } catch (error) {
      renderDisconnected(error);
    }
  });
});

els.applyMode.classList.add("ghost");
els.applyMode.addEventListener("click", async () => {
  try {
    await sendAction({ SetMode: els.modeSelect.value });
    const snapshot = await fetchHealth();
    render(snapshot);
  } catch (error) {
    renderDisconnected(error);
  }
});

async function refreshLoop() {
  try {
    const snapshot = await fetchHealth();
    render(snapshot);
  } catch (error) {
    renderDisconnected(error);
  } finally {
    window.setTimeout(refreshLoop, 900);
  }
}

refreshLoop();
