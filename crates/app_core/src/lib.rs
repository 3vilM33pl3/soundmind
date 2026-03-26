use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use dotenvy::from_filename_override;
use ipc_schema::{
    AppMode, AppSettingsDto, BackendStatusSnapshot, CaptureState, CloudState, PrimingDocumentDto,
    SessionDetailDto, SessionSummaryDto, UserAction, LlmModelDescriptorDto,
    default_assistant_instruction,
    default_llm_model, default_llm_provider,
};
use serde::{Deserialize, Serialize};
use storage_sqlite::Storage;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::{RwLock, mpsc};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct RootConfig {
    pub app: AppSection,
    pub capture: CaptureSection,
    pub storage: StorageSection,
    pub providers: ProviderSection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppSection {
    pub mode: AppMode,
    pub auto_start: bool,
    #[serde(default = "default_auto_start_cloud")]
    pub auto_start_cloud: bool,
    pub http_bind: String,
    pub simulate_transcriber: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptureSection {
    pub source: String,
    pub frame_ms: u64,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub silence_threshold: f32,
    pub chunk_ms: u64,
    #[serde(default = "default_speech_hold_ms")]
    pub speech_hold_ms: u64,
    #[serde(default = "default_idle_timeout_ms")]
    pub idle_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageSection {
    pub database_path: String,
    pub retention_days: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderSection {
    pub openai: ProviderConfig,
    pub elevenlabs: ProviderConfig,
    #[serde(default)]
    pub ollama: LocalProviderConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub enabled: bool,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ollama_model")]
    pub model: String,
    #[serde(default = "default_ollama_base_url")]
    pub base_url: String,
}

impl Default for LocalProviderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_ollama_model(),
            base_url: default_ollama_base_url(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionExportQuery {
    pub format: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadPrimingDocumentRequest {
    pub file_name: String,
    pub mime_type: Option<String>,
    pub content_base64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeletePrimingDocumentResponse {
    pub deleted: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub struct SessionExport {
    pub content_type: &'static str,
    pub body: String,
}

#[derive(Clone)]
pub struct AppCoreState {
    snapshot: Arc<RwLock<BackendStatusSnapshot>>,
    action_tx: mpsc::Sender<UserAction>,
    storage: Arc<Storage>,
    settings: Arc<RwLock<AppSettingsDto>>,
    llm_models: Arc<Vec<LlmModelDescriptorDto>>,
}

impl AppCoreState {
    pub async fn initialize(
        config: &RootConfig,
        storage: Arc<Storage>,
        action_tx: mpsc::Sender<UserAction>,
        llm_models: Vec<LlmModelDescriptorDto>,
    ) -> Result<Self> {
        let settings = Arc::new(RwLock::new(load_or_initialize_settings(&storage, config).await?));
        let initial_settings = settings.read().await.clone();
        let snapshot = Arc::new(RwLock::new(BackendStatusSnapshot {
            mode: initial_settings.default_mode,
            capture_state: if config.app.auto_start {
                CaptureState::Capturing
            } else {
                CaptureState::Paused
            },
            cloud_state: CloudState::Off,
            privacy_pause: !config.app.auto_start,
            cloud_pause: !initial_settings.auto_start_cloud,
            ..BackendStatusSnapshot::default()
        }));

        Ok(Self { snapshot, action_tx, storage, settings, llm_models: Arc::new(llm_models) })
    }

    pub fn snapshot_handle(&self) -> Arc<RwLock<BackendStatusSnapshot>> {
        Arc::clone(&self.snapshot)
    }

    pub fn settings_handle(&self) -> Arc<RwLock<AppSettingsDto>> {
        Arc::clone(&self.settings)
    }

    pub fn storage_handle(&self) -> Arc<Storage> {
        Arc::clone(&self.storage)
    }

    pub fn llm_models(&self) -> Vec<LlmModelDescriptorDto> {
        self.llm_models.as_ref().clone()
    }

    pub async fn snapshot(&self) -> BackendStatusSnapshot {
        let mut snapshot = self.snapshot.read().await.clone();
        snapshot.recent_errors.retain(|error| !error.trim().is_empty());
        snapshot
    }

    pub async fn dispatch_action(&self, action: UserAction) -> Result<()> {
        self.action_tx.send(action).await.context("failed to dispatch user action")
    }

    pub async fn get_settings(&self) -> AppSettingsDto {
        self.settings.read().await.clone()
    }

    pub async fn save_settings(&self, mut settings: AppSettingsDto) -> Result<AppSettingsDto> {
        if settings.assistant_instruction.trim().is_empty() {
            settings.assistant_instruction = default_assistant_instruction();
        }
        if settings.llm_provider.trim().is_empty() {
            settings.llm_provider = default_llm_provider();
        }
        if settings.llm_model.trim().is_empty() {
            settings.llm_model = default_llm_model();
        }
        self.storage.save_settings(&settings).await?;
        *self.settings.write().await = settings.clone();
        Ok(settings)
    }

    pub async fn list_priming_documents(&self) -> Result<Vec<PrimingDocumentDto>> {
        self.storage.list_priming_documents().await
    }

    pub async fn upload_priming_document(
        &self,
        request: UploadPrimingDocumentRequest,
    ) -> Result<PrimingDocumentDto> {
        let decoded = BASE64_STANDARD
            .decode(request.content_base64.as_bytes())
            .context("priming document was not valid base64")?;
        let mime_type = request
            .mime_type
            .clone()
            .filter(|mime_type| !mime_type.trim().is_empty())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let extracted_text = extract_document_text(&request.file_name, &mime_type, &decoded).await?;
        let document = self
            .storage
            .insert_priming_document(&request.file_name, &mime_type, &extracted_text)
            .await?;
        self.storage
            .append_audit_event(None, &format!("priming_document_added:{}", request.file_name))
            .await
            .ok();
        Ok(document)
    }

    pub async fn delete_priming_document(
        &self,
        document_id: Uuid,
    ) -> Result<DeletePrimingDocumentResponse> {
        self.storage.delete_priming_document(document_id).await?;
        self.storage
            .append_audit_event(None, &format!("priming_document_deleted:{document_id}"))
            .await
            .ok();
        Ok(DeletePrimingDocumentResponse { deleted: document_id })
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummaryDto>> {
        self.storage.list_sessions(100).await
    }

    pub async fn get_session_detail(&self, session_id: Uuid) -> Result<Option<SessionDetailDto>> {
        self.storage.get_session_detail(session_id).await
    }

    pub async fn delete_session(&self, session_id: Uuid) -> Result<()> {
        if self.snapshot.read().await.session_id == Some(session_id) {
            anyhow::bail!("cannot delete the currently active session");
        }
        self.storage.delete_session(session_id).await
    }

    pub async fn purge_sessions(&self) -> Result<u64> {
        let retention_days = self.settings.read().await.retention_days;
        self.storage.purge_sessions_older_than_days(retention_days).await
    }

    pub async fn export_session(
        &self,
        session_id: Uuid,
        format: Option<&str>,
    ) -> Result<Option<SessionExport>> {
        let Some(session) = self.storage.get_session_detail(session_id).await? else {
            return Ok(None);
        };

        let exported = match format.unwrap_or("json") {
            "markdown" | "md" => SessionExport {
                content_type: "text/markdown; charset=utf-8",
                body: render_session_markdown(&session),
            },
            _ => SessionExport {
                content_type: "application/json",
                body: serde_json::to_string_pretty(&session)?,
            },
        };
        Ok(Some(exported))
    }
}

pub fn load_config() -> Result<RootConfig> {
    let path = resolve_config_path()?;
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut config: RootConfig =
        toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))?;

    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        if !api_key.trim().is_empty() {
            config.providers.openai.enabled = true;
        }
    }

    if let Ok(api_key) = std::env::var("ELEVENLABS_API_KEY") {
        if !api_key.trim().is_empty() {
            config.providers.elevenlabs.enabled = true;
            if config.app.simulate_transcriber {
                info!(
                    "ELEVENLABS_API_KEY is present; overriding simulate_transcriber=true to use the live provider by default"
                );
                config.app.simulate_transcriber = false;
            }
        }
    }

    if let Ok(model) = std::env::var("ELEVENLABS_MODEL") {
        if !model.trim().is_empty() {
            config.providers.elevenlabs.model = model;
        }
    } else if config.providers.elevenlabs.model == "scribe_v1" {
        info!(
            "Overriding legacy realtime STT model scribe_v1 with scribe_v2_realtime for the ElevenLabs realtime endpoint"
        );
        config.providers.elevenlabs.model = "scribe_v2_realtime".to_string();
    }

    info!(
        retention_days = if config.storage.retention_days == 0 {
            "infinite".to_string()
        } else {
            config.storage.retention_days.to_string()
        },
        auto_start_cloud = config.app.auto_start_cloud,
        simulate_transcriber = config.app.simulate_transcriber,
        "loaded configuration"
    );
    Ok(config)
}

pub fn load_keys_env() {
    for path in candidate_keys_env_paths() {
        if path.exists() {
            if let Err(error) = from_filename_override(&path) {
                warn!(path = %path.display(), ?error, "failed to load keys.env");
            } else {
                info!(path = %path.display(), "loaded provider keys");
            }
            break;
        }
    }
}

pub fn sqlite_url(path: &str) -> String {
    if path.starts_with("sqlite:") { path.to_string() } else { format!("sqlite://{path}") }
}

fn default_auto_start_cloud() -> bool {
    false
}

fn default_speech_hold_ms() -> u64 {
    1_200
}

fn default_idle_timeout_ms() -> u64 {
    5_000
}

fn default_ollama_model() -> String {
    "llama3.2:3b-instruct-q4_K_M".to_string()
}

fn default_ollama_base_url() -> String {
    "http://127.0.0.1:11434".to_string()
}

async fn load_or_initialize_settings(
    storage: &Storage,
    config: &RootConfig,
) -> Result<AppSettingsDto> {
    if let Some(settings) = storage.load_settings().await? {
        return Ok(settings);
    }

    let settings = AppSettingsDto {
        retention_days: config.storage.retention_days,
        transcript_storage_enabled: true,
        auto_start_cloud: config.app.auto_start_cloud,
        default_mode: config.app.mode,
        llm_provider: default_llm_provider_for_config(config),
        llm_model: default_llm_model_for_config(config),
        assistant_instruction: default_assistant_instruction(),
    };
    storage.save_settings(&settings).await?;
    Ok(settings)
}

fn default_llm_provider_for_config(config: &RootConfig) -> String {
    if config.providers.openai.enabled {
        "openai".to_string()
    } else if config.providers.ollama.enabled {
        "ollama".to_string()
    } else {
        default_llm_provider()
    }
}

fn default_llm_model_for_config(config: &RootConfig) -> String {
    if config.providers.openai.enabled && !config.providers.openai.model.trim().is_empty() {
        config.providers.openai.model.clone()
    } else if config.providers.ollama.enabled && !config.providers.ollama.model.trim().is_empty() {
        config.providers.ollama.model.clone()
    } else {
        default_llm_model()
    }
}

async fn extract_document_text(file_name: &str, mime_type: &str, bytes: &[u8]) -> Result<String> {
    let extension =
        file_name.rsplit('.').next().map(|value| value.to_ascii_lowercase()).unwrap_or_default();

    let text = if is_text_like_mime(mime_type)
        || matches!(
            extension.as_str(),
            "txt" | "md" | "markdown" | "json" | "html" | "htm" | "csv" | "log" | "yaml" | "yml"
        ) {
        String::from_utf8(bytes.to_vec()).context("document is not valid UTF-8 text")?
    } else if mime_type == "application/pdf" || extension == "pdf" {
        extract_pdf_text(bytes).await?
    } else {
        anyhow::bail!(
            "Unsupported document format. Upload text, markdown, JSON, HTML, CSV, YAML, or PDF."
        );
    };

    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        anyhow::bail!("The uploaded document did not contain usable text.");
    }
    if trimmed.chars().count() > 120_000 {
        anyhow::bail!("The uploaded document is too large after text extraction.");
    }

    Ok(trimmed.to_string())
}

fn is_text_like_mime(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || matches!(
            mime_type,
            "application/json"
                | "application/ld+json"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/yaml"
                | "application/x-yaml"
        )
}

async fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    let temp_path = std::env::temp_dir().join(format!("soundmind-upload-{}.pdf", Uuid::new_v4()));
    fs::write(&temp_path, bytes).await.context("failed to stage uploaded PDF")?;

    let output =
        match Command::new("pdftotext").arg("-layout").arg(&temp_path).arg("-").output().await {
            Ok(output) => output,
            Err(error) => {
                let _ = fs::remove_file(&temp_path).await;
                if error.kind() == std::io::ErrorKind::NotFound {
                    anyhow::bail!(
                        "PDF upload requires `pdftotext` to be installed on this machine."
                    );
                }
                return Err(error).context("failed to run pdftotext");
            }
        };
    let _ = fs::remove_file(&temp_path).await;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("Failed to extract PDF text: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn render_session_markdown(session: &SessionDetailDto) -> String {
    let mut markdown = String::new();
    markdown.push_str("# Soundmind Session Export\n\n");
    markdown.push_str(&format!("Session ID: `{}`\n", session.session.id));
    markdown.push_str(&format!("Started: {}\n", session.session.started_at.to_rfc3339()));
    if let Some(ended_at) = session.session.ended_at {
        markdown.push_str(&format!("Ended: {}\n", ended_at.to_rfc3339()));
    }
    markdown.push_str(&format!("Capture Device: {}\n", session.session.capture_device));
    markdown.push_str(&format!("Mode: {}\n\n", session.session.mode));
    markdown.push_str("## Transcript\n\n");
    for segment in &session.transcript_segments {
        markdown.push_str(&format!(
            "- [{}-{} ms] {}\n",
            segment.start_ms, segment.end_ms, segment.text
        ));
    }
    markdown.push_str("\n## Assistant Events\n\n");
    for event in &session.assistant_events {
        markdown
            .push_str(&format!("- {} ({:.2}): {}\n", event.kind, event.confidence, event.content));
    }
    markdown
}

fn resolve_config_path() -> Result<PathBuf> {
    for path in candidate_config_paths() {
        if path.exists() {
            return Ok(path);
        }
    }

    let searched = candidate_config_paths()
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("no config file found; searched {searched}")
}

fn candidate_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("SOUNDMIND_CONFIG") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit));
        }
    }
    paths.push(PathBuf::from("config.toml"));
    if let Some(config_dir) = soundmind_config_dir() {
        paths.push(config_dir.join("config.toml"));
    }
    paths.push(PathBuf::from("config.example.toml"));
    paths
}

fn candidate_keys_env_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(explicit) = std::env::var("SOUNDMIND_KEYS_ENV") {
        if !explicit.trim().is_empty() {
            paths.push(PathBuf::from(explicit));
        }
    }
    paths.push(PathBuf::from("keys.env"));
    if let Some(config_dir) = soundmind_config_dir() {
        paths.push(config_dir.join("keys.env"));
    }
    paths
}

fn soundmind_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|path| path.join("soundmind"))
}
