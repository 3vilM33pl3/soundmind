// @ts-check

export const DEFAULT_BACKEND_URL = "http://127.0.0.1:8765";

export function createApi(backendUrl) {
  async function fetchJson(path, init = undefined) {
    const response = await fetch(`${backendUrl}${path}`, init);
    if (!response.ok) {
      throw new Error(`${path} returned ${response.status}`);
    }
    return response.json();
  }

  return {
    backendUrl,
    fetchHealth: () => fetchJson("/health"),
    fetchSettings: () => fetchJson("/settings"),
    fetchLlmModels: () => fetchJson("/llm/models"),
    fetchSessions: () => fetchJson("/sessions"),
    fetchPrimingDocuments: () => fetchJson("/priming-documents"),
    fetchSessionDetail: (sessionId) => fetchJson(`/sessions/${sessionId}`),
    sendAction: (action) =>
      fetchJson("/actions", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(action),
      }),
    putSettings: (settings) =>
      fetchJson("/settings", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(settings),
      }),
    uploadPrimingDocument: (document) =>
      fetchJson("/priming-documents", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(document),
      }),
    deletePrimingDocument: (documentId) =>
      fetchJson(`/priming-documents/${documentId}`, { method: "DELETE" }),
    purgeHistory: () => fetchJson("/sessions/purge", { method: "POST" }),
    deleteSession: (sessionId) => fetchJson(`/sessions/${sessionId}`, { method: "DELETE" }),
    exportSession: async (sessionId, format) => {
      const response = await fetch(`${backendUrl}/sessions/${sessionId}/export?format=${format}`);
      if (!response.ok) {
        throw new Error(`export failed with ${response.status}`);
      }
      return response.text();
    },
  };
}
