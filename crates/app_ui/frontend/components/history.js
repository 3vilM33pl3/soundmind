// @ts-check

import { RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY } from "../lib/app_state.js";
import { escapeHtml, formatTime } from "../lib/utils.js";
import { clearTranscriptSelection, renderTranscriptPanel } from "./transcript.js";
import { runWithButtonFeedback } from "../lib/ui.js";

export function renderSessions(state, els, app) {
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
        await app.selectSession(sessionId);
      } catch (error) {
        app.renderDisconnected(error);
      }
    });
  });
}

export function renderSessionDetail(state, els, app) {
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
              <div class="segment-meta">
                ${escapeHtml(formatAssistantEventKind(event))} • ${formatTime(event.created_at)}
                ${event.model_id ? `<span class="assistant-model">${escapeHtml(event.model_id)}</span>` : ""}
                ${event.reused_from_history ? `<span class="reuse-badge">From History</span>` : ""}
              </div>
              ${event.request_text ? `<div class="segment-meta detail-request">${escapeHtml(event.request_text)}</div>` : ""}
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
    clearTranscriptSelection(state, els);
    renderTranscriptPanel(state, els, app);
    els.segmentList.scrollTop = 0;
  });
  document.querySelector("#export-json")?.addEventListener("click", async () => {
    const button = document.querySelector("#export-json");
    try {
      await runWithButtonFeedback(
        button,
        () => app.handleExport(session.session.id, "json"),
        { pending: "Exporting...", success: "JSON Ready", error: "Export Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });
  document.querySelector("#export-markdown")?.addEventListener("click", async () => {
    const button = document.querySelector("#export-markdown");
    try {
      await runWithButtonFeedback(
        button,
        () => app.handleExport(session.session.id, "markdown"),
        { pending: "Exporting...", success: "Markdown Ready", error: "Export Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
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
          await app.api.deleteSession(session.session.id);
          state.selectedSessionId = null;
          state.selectedSession = null;
          state.restoredTranscriptSessionId = null;
          window.localStorage.removeItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY);
          await app.refreshHistory(true);
          els.settingsNote.textContent = "Session deleted.";
        },
        { pending: "Deleting...", success: "Deleted", error: "Delete Failed" },
      );
    } catch (error) {
      app.renderDisconnected(error);
    }
  });
}

function formatAssistantEventKind(event) {
  const requestKind = (event.request_kind || "").trim();
  switch (event.kind) {
    case "manual_answer":
      return "Manual answer";
    case "manual_summary":
      return "Manual summary";
    case "manual_commentary":
      return "Manual commentary";
    case "automatic_answer":
      return "Automatic answer";
    case "automatic_summary":
      return "Automatic summary";
    case "automatic_commentary":
      return "Automatic commentary";
    default:
      if (requestKind === "answer") return "Answer";
      if (requestKind === "summary") return "Summary";
      if (requestKind === "commentary") return "Commentary";
      return event.kind || "Assistant";
  }
}

