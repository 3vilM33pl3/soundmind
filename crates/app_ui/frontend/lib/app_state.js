// @ts-check

export const RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY = "soundmind.restoredTranscriptSessionId";

export function createState() {
  return {
    snapshot: null,
    settings: null,
    availableLlmModels: [],
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
    manualQuestionSelections: [],
    manualQuestionSelectionSessionId: null,
    restoredTranscriptSessionId: window.localStorage.getItem(RESTORED_TRANSCRIPT_SESSION_STORAGE_KEY),
  };
}

export function isRestoredTranscriptView(state) {
  return Boolean(
    state.restoredTranscriptSessionId &&
      state.selectedSession &&
      state.selectedSession.session.id === state.restoredTranscriptSessionId,
  );
}

export function currentTranscriptSegments(state) {
  if (isRestoredTranscriptView(state)) {
    return state.selectedSession?.transcript_segments || [];
  }
  return state.snapshot?.transcript?.segments || [];
}

export function currentTranscriptSessionId(state) {
  return isRestoredTranscriptView(state)
    ? state.selectedSession?.session?.id || null
    : state.snapshot?.session_id || null;
}

export function currentManualQuestionSelections(state) {
  if (!state.manualQuestionSelections.length || !state.manualQuestionSelectionSessionId) {
    return [];
  }

  const currentSessionId = currentTranscriptSessionId(state);
  if (!currentSessionId || String(currentSessionId) !== String(state.manualQuestionSelectionSessionId)) {
    return [];
  }

  return state.manualQuestionSelections;
}

export function currentManualQuestionSelection(state) {
  const selections = currentManualQuestionSelections(state);
  return selections.length ? selections[selections.length - 1] : null;
}

export function currentActionSelection(state) {
  return (
    state.transcriptSelection ||
    state.stickyTranscriptSelection ||
    currentManualQuestionSelection(state)
  );
}

export function currentDetectedQuestion(state) {
  const promotedSelection = currentManualQuestionSelection(state);
  if (promotedSelection?.selected_text) {
    return {
      text: promotedSelection.selected_text,
      start_ms: null,
      end_ms: null,
      manual: true,
    };
  }

  if (isRestoredTranscriptView(state)) {
    return [...currentTranscriptSegments(state)]
      .reverse()
      .find((segment) => segment.is_question_candidate) || null;
  }
  return state.snapshot?.detected_question || null;
}

export function promoteManualQuestionSelection(state, selection) {
  if (!selection || !Array.isArray(selection.segment_ids) || !selection.segment_ids.length) {
    return;
  }

  const promotedSelection = {
    selected_text: String(selection.selected_text || "").trim(),
    segment_ids: [...new Set(selection.segment_ids)],
  };
  const existingSelections = currentManualQuestionSelections(state).filter(
    (candidate) => candidate.segment_ids.join(",") !== promotedSelection.segment_ids.join(","),
  );
  state.manualQuestionSelections = [...existingSelections, promotedSelection];
  state.transcriptSelection = null;
  state.stickyTranscriptSelection = promotedSelection;
  state.manualQuestionSelectionSessionId = currentTranscriptSessionId(state);
  state.lastManualQuestionSelectionKey = null;
}

export function clearManualQuestionSelection(state) {
  state.manualQuestionSelections = [];
  state.manualQuestionSelectionSessionId = null;
  state.lastManualQuestionSelectionKey = null;
}
