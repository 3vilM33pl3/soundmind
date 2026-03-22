CREATE TABLE IF NOT EXISTS priming_documents (
  id TEXT PRIMARY KEY,
  file_name TEXT NOT NULL,
  mime_type TEXT NOT NULL,
  extracted_text TEXT NOT NULL,
  created_at TEXT NOT NULL
);
