// @ts-check

import { currentActionSelection, currentDetectedQuestion, currentManualQuestionSelection } from "../lib/app_state.js";
import { contentToBulletCandidates, escapeHtml, formatTime } from "../lib/utils.js";

export function renderAssistantCards(state, els) {
  renderAssistantCard(els.manualAssistantCard, state.snapshot?.manual_assistant, {
    emptyMeta: "No manual response yet.",
    emptyContent: "Trigger an action once transcript is available.",
    fallbackQuestion: currentManualQuestionContext(state),
  });

  renderAssistantCard(els.automaticAssistantCard, state.snapshot?.automatic_assistant, {
    emptyMeta: "No assisted question answer yet.",
    emptyContent:
      state.snapshot?.mode === "Assisted"
        ? "Waiting for a newly committed detected question."
        : "Switch to Assisted mode to auto-answer the latest detected question.",
    fallbackQuestion: state.snapshot?.detected_question?.text?.trim() || null,
    emptyQuestion: state.snapshot?.detected_question?.text?.trim() || null,
  });
}

export function renderAssistantContent(content, kind = "Notice") {
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

export function renderAssistantCard(cardElement, assistant, options) {
  if (!cardElement) {
    return;
  }

  if (!assistant) {
    const emptyQuestionMarkup = options.emptyQuestion
      ? `
        <div class="assistant-question">
          <div class="assistant-question-label">Question</div>
          <div class="assistant-question-text">${escapeHtml(options.emptyQuestion)}</div>
        </div>
      `
      : "";
    cardElement.innerHTML = `
      <div class="assistant-meta">${escapeHtml(options.emptyMeta)}</div>
      ${emptyQuestionMarkup}
      <div class="assistant-content">${escapeHtml(options.emptyContent)}</div>
    `;
    return;
  }

  const questionContext = assistant.question_text?.trim() || options.fallbackQuestion || null;
  const questionLabel = assistant.kind === "Answer" ? "Question" : "Focus";
  const questionMarkup = questionContext
    ? `
      <div class="assistant-question">
        <div class="assistant-question-label">${questionLabel}</div>
        <div class="assistant-question-text">${escapeHtml(questionContext)}</div>
      </div>
    `
    : "";
  const cachedBadge = assistant.reused_from_history
    ? `<span class="reuse-badge">From History</span>`
    : "";
  const modelLabel = assistant.source_model
    ? `<span class="assistant-model">${escapeHtml(assistant.source_model)}</span>`
    : "";

  cardElement.innerHTML = `
    <div class="assistant-meta">
      ${escapeHtml(assistant.kind)} • ${formatTime(assistant.created_at)} ${modelLabel} ${cachedBadge}
    </div>
    ${questionMarkup}
    <div class="assistant-content">${renderAssistantContent(assistant.content, assistant.kind)}</div>
  `;
}

export function currentManualQuestionContext(state) {
  const selection = currentManualQuestionSelection(state) || currentActionSelection(state);
  if (selection && selection.selected_text.trim()) {
    return selection.selected_text.trim();
  }

  const detectedQuestion = currentDetectedQuestion(state);
  return detectedQuestion?.text?.trim() || null;
}

