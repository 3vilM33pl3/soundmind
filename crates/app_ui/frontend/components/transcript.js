// @ts-check

import {
  clearManualQuestionSelection,
  currentActionSelection,
  currentDetectedQuestion,
  currentManualQuestionSelection,
  currentManualQuestionSelections,
  currentTranscriptSegments,
  currentTranscriptSessionId,
  isRestoredTranscriptView,
  promoteManualQuestionSelection,
} from "../lib/app_state.js";
import { escapeHtml, normalizeTranscriptText } from "../lib/utils.js";
import { runWithButtonFeedback } from "../lib/ui.js";

export function bindTranscriptInteractions(state, els, app) {
  document.querySelectorAll("[data-question-segment-id]").forEach((button) => {
    button.addEventListener("click", async () => {
      const segmentId = button.dataset.questionSegmentId;
      if (!segmentId) {
        return;
      }

      if (isRestoredTranscriptView(state)) {
        const segment = currentTranscriptSegments(state).find(
          (candidate) => String(candidate.id) === String(segmentId),
        );
        if (!segment) {
          return;
        }
        state.transcriptSelection = { selected_text: segment.text, segment_ids: [segment.id] };
        state.stickyTranscriptSelection = state.transcriptSelection;
        renderSelectionState(state, els);
        return;
      }

      const segment = currentTranscriptSegments(state).find(
        (candidate) => String(candidate.id) === String(segmentId),
      );
      const promotedSelection = segment
        ? { selected_text: segment.text, segment_ids: [segment.id] }
        : null;

      try {
        await runWithButtonFeedback(
          button,
          async () => {
            await app.api.sendAction({ AnswerQuestionBySegment: { segment_id: segmentId } });
            const snapshot = await app.api.fetchHealth();
            app.renderSnapshot(snapshot);
            if (promotedSelection) {
              promoteManualQuestionSelection(state, promotedSelection);
              renderTranscriptPanel(state, els, app);
            }
          },
          { pending: "Answering...", success: "Answer Ready", error: "Answer Failed" },
        );
      } catch (error) {
        app.renderDisconnected(error);
      }
    });
  });
}

export function updateTranscriptSelection(state, els) {
  const nextSelection = snapshotTranscriptSelection(els);
  if (!nextSelection) {
    state.transcriptSelection = null;
    renderSelectionState(state, els);
    return;
  }

  state.transcriptSelection = nextSelection;
  state.stickyTranscriptSelection = nextSelection;
  renderSelectionState(state, els);
}

export function refreshCurrentSelectionTarget(state, els) {
  const selection = snapshotTranscriptSelection(els);
  if (!selection) {
    return null;
  }

  state.transcriptSelection = selection;
  state.stickyTranscriptSelection = selection;
  renderSelectionState(state, els);
  return selection;
}

export function clearTranscriptSelection(state, els) {
  const selection = window.getSelection();
  if (selection) {
    selection.removeAllRanges();
  }
  state.transcriptSelection = null;
  state.stickyTranscriptSelection = null;
  clearManualQuestionSelection(state);
  renderSelectionState(state, els);
}

export function renderSelectionState(state, els) {
  const selection = currentActionSelection(state);
  const promotedSelection = currentManualQuestionSelection(state);
  const restoredTranscript = isRestoredTranscriptView(state);

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
  if (
    promotedSelection &&
    selection &&
    selection.selected_text !== promotedSelection.selected_text
  ) {
    els.selectionStatus.textContent = selection.segment_ids.length
      ? `New selection active across ${selection.segment_ids.length} segment${selection.segment_ids.length === 1 ? "" : "s"}. Top actions will target it, while the previous answered highlight stays visible.`
      : "New selection active. Top actions will target it, while the previous answered highlight stays visible.";
  } else {
    els.selectionStatus.textContent = selection.segment_ids.length
      ? `Selection active across ${selection.segment_ids.length} segment${selection.segment_ids.length === 1 ? "" : "s"}. Top actions now target the selected excerpt.`
      : "Selection active in the live transcript text. Top actions now target the selected excerpt.";
  }
  els.clearSelectionButton.disabled = false;
  els.actionAnswerButton.disabled = false;
  els.actionSummaryButton.disabled = false;
  els.actionCommentButton.disabled = false;
  els.actionAnswerButton.textContent = "Answer Selection";
  els.actionSummaryButton.textContent = "Summarize Selection";
  els.actionCommentButton.textContent = "Comment on Selection";
}

export function renderTranscriptPanel(state, els, app) {
  const transcriptScrollState = captureTranscriptScrollState(els);
  const restoredTranscript = isRestoredTranscriptView(state);
  const segments = currentTranscriptSegments(state);
  const partialText = restoredTranscript
    ? ""
    : normalizeTranscriptText(state.snapshot?.transcript?.partial_text || "");
  const manualSelections = currentManualQuestionSelections(state);
  const manualSelectionKey = manualSelections.length
    ? `${state.manualQuestionSelectionSessionId || ""}:${manualSelections
        .map((selection) => `${selection.segment_ids.join(",")}:${selection.selected_text}`)
        .join("|")}`
    : "";
  const transcriptRenderKey = buildTranscriptRenderKey(state, segments, restoredTranscript, manualSelectionKey);
  const questionButtonTitle = restoredTranscript ? "Select this question" : "Answer this question";

  els.transcriptHint.textContent = restoredTranscript
    ? "Viewing a stored session transcript. Scroll through it, click `?` to select a detected question, or return to the live transcript."
    : "Scroll continuously, click `?` beside detected questions, or select text and use the action bar above.";
  els.transcriptReturnLive.hidden = !restoredTranscript;
  els.partialBox.textContent = restoredTranscript
    ? `Stored session restored. ${segments.length} committed transcript segment${segments.length === 1 ? "" : "s"} available.`
    : partialText || "No partial transcript yet.";

  renderQuestionBanner(state, els, currentDetectedQuestion(state), restoredTranscript);
  renderSelectionState(state, els);

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
      manualSelections,
      questionButtonTitle,
    });

    els.segmentList.innerHTML = transcriptMarkup;
    bindTranscriptInteractions(state, els, app);
    state.lastTranscriptRenderKey = transcriptRenderKey;
  } else {
    updateLivePartialText(partialText);
  }

  state.lastTranscriptPartialText = partialText;
  state.lastManualQuestionSelectionKey = manualSelectionKey;
  restoreTranscriptScrollState(els, transcriptScrollState);
}

function snapshotTranscriptSelection(els) {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0 || selection.isCollapsed) {
    return null;
  }

  const range = selection.getRangeAt(0);
  if (!selectionIsInsideTranscript(els, range)) {
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

function selectionIsInsideTranscript(els, range) {
  const container = els.segmentList;
  if (!container) {
    return false;
  }

  const commonAncestor = range.commonAncestorContainer;
  return commonAncestor instanceof Element
    ? container.contains(commonAncestor)
    : container.contains(commonAncestor.parentElement);
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

function buildTranscriptRenderKey(state, segments, restoredTranscript, manualSelectionKey) {
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

function renderTranscriptMarkup({ segments, partialText, restoredTranscript, manualSelections, questionButtonTitle }) {
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
              ${renderTranscriptParagraph(paragraph, manualSelections, questionButtonTitle)}
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

function renderTranscriptParagraph(paragraph, manualSelections, questionButtonTitle) {
  const manualSelectionBySegmentId = new Map();
  for (const selection of manualSelections || []) {
    for (const segmentId of selection.segment_ids || []) {
      manualSelectionBySegmentId.set(String(segmentId), selection);
    }
  }
  const parts = [];

  for (let index = 0; index < paragraph.length; index += 1) {
    const segment = paragraph[index];
    const segmentId = String(segment.id);
    const manualSelection = manualSelectionBySegmentId.get(segmentId);

    if (manualSelection) {
      const run = [segment];
      while (
        index + 1 < paragraph.length &&
        manualSelectionBySegmentId.get(String(paragraph[index + 1].id)) === manualSelection
      ) {
        index += 1;
        run.push(paragraph[index]);
      }
      parts.push(renderManualTranscriptRun(run, manualSelection, questionButtonTitle));
      continue;
    }

    parts.push(renderStandardTranscriptSegment(segment, questionButtonTitle));
  }

  return parts.join(" ");
}

function renderManualTranscriptRun(run, manualSelection, questionButtonTitle) {
  const manualAnchorId = manualSelection?.segment_ids?.length
    ? String(manualSelection.segment_ids[manualSelection.segment_ids.length - 1])
    : null;
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

function captureTranscriptScrollState(els) {
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

function restoreTranscriptScrollState(els, previousState) {
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

function renderQuestionBanner(state, els, question, restoredTranscript = false) {
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
      <div class="question-label">${question.manual ? "Selected excerpt" : "Stored question"}</div>
      <div class="question-body">${escapeHtml(question.text)}</div>
      ${question.manual ? "" : `<div class="question-meta">${question.start_ms}-${question.end_ms} ms</div>`}
    `;
    return;
  }

  if (!question) {
    els.questionBanner.className = "question-banner question-banner-idle";
    els.questionBanner.innerHTML = `
      <div class="question-label">Question status</div>
      <div class="question-body">No question detected right now.</div>
    `;
    if (!currentActionSelection(state)) {
      els.actionAnswerButton.textContent = "Answer Last Question";
    }
    return;
  }

  els.questionBanner.className = "question-banner question-banner-detected";
  els.questionBanner.innerHTML = `
    <div class="question-label">${question.manual ? "Selected excerpt" : "Question detected"}</div>
    <div class="question-body">${escapeHtml(question.text)}</div>
    ${question.manual ? "" : `<div class="question-meta">${question.start_ms}-${question.end_ms} ms</div>`}
  `;
  if (!currentActionSelection(state)) {
    els.actionAnswerButton.textContent = question.manual ? "Answer Selection" : "Answer Detected Question";
  }
}

