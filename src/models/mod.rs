use serde::{Deserialize, Serialize};

// Phase enum for event types
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Phase {
    #[serde(rename = "import:start")]
    ImportStart,
    #[serde(rename = "import:copying")]
    ImportCopying,
    #[serde(rename = "import:complete")]
    ImportComplete,
    #[serde(rename = "import:error")]
    ImportError,
    #[serde(rename = "create:start")]
    CreateStart,
    #[serde(rename = "create:downloading")]
    CreateDownloading,
    #[serde(rename = "create:copying")]
    CreateCopying,
    #[serde(rename = "create:complete")]
    CreateComplete,
    #[serde(rename = "create:error")]
    CreateError,
    #[serde(rename = "download:start")]
    DownloadStart,
    #[serde(rename = "download:progress")]
    DownloadProgress,
    #[serde(rename = "download:complete")]
    DownloadComplete,
    #[serde(rename = "download:error")]
    DownloadError,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "cancel")]
    Cancel,
}

impl Phase {
    /// Returns the string representation for the phase
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::ImportStart => "import:start",
            Phase::ImportCopying => "import:copying",
            Phase::ImportComplete => "import:complete",
            Phase::ImportError => "import:error",
            Phase::CreateStart => "create:start",
            Phase::CreateDownloading => "create:downloading",
            Phase::CreateCopying => "create:copying",
            Phase::CreateComplete => "create:complete",
            Phase::CreateError => "create:error",
            Phase::DownloadStart => "download:start",
            Phase::DownloadProgress => "download:progress",
            Phase::DownloadComplete => "download:complete",
            Phase::DownloadError => "download:error",
            Phase::Cancelled => "cancelled",
            Phase::Cancel => "cancel",
        }
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}


#[derive(Serialize)]
pub struct OpenProjectResponse {
    pub launched: bool,
    pub engine_name: Option<String>,
    pub engine_version: Option<String>,
    pub editor_path: Option<String>,
    pub project: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct OpenEngineResponse {
    pub launched: bool,
    pub engine_name: Option<String>,
    pub engine_version: Option<String>,
    pub editor_path: Option<String>,
    pub message: String,
}


/// Request payload for importing a downloaded asset into a UE project.
#[derive(serde::Deserialize)]
pub struct ImportAssetRequest {
    /// Asset folder name as stored under downloads/ (e.g., "Industry Props Pack 6").
    /// If namespace/asset_id/artifact_id are provided, this can be ignored; the server
    /// will derive the actual download folder name from Fab metadata.
    pub asset_name: String,
    /// Optional Fab identifiers to trigger a download prior to import.
    /// When provided, the server will reuse the same logic as /download-asset.
    pub namespace: Option<String>,
    pub asset_id: Option<String>,
    pub artifact_id: Option<String>,
    /// Optional Unreal Engine major.minor version subfolder (e.g., "5.4").
    pub ue: Option<String>,
    /// Project identifier: name, project directory, or path to .uproject
    pub project: String,
    /// Optional subfolder inside Project/Content to copy into (e.g., "Imported/Industry").
    pub target_subdir: Option<String>,
    /// When true, overwrite existing files. When false, skip existing files.
    pub overwrite: Option<bool>,
    /// Optional job id to stream progress over WebSocket
    pub job_id: Option<String>,
}

#[derive(Serialize)]
pub struct ImportAssetResponse {
    pub ok: bool,
    pub message: String,
    pub files_copied: usize,
    pub files_skipped: usize,
    pub source: String,
    pub destination: String,
    pub elapsed_ms: u128,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateUnrealProjectRequest {
    pub engine_path: Option<String>,
    /// Path to a template/sample .uproject OR a directory containing one. If omitted, provide asset_name.
    pub template_project: Option<String>,
    /// Convenience: name of a downloaded asset under downloads/ (e.g., "Stack O Bot").
    /// When provided and template_project is empty, the server will search downloads/<asset_name>/ recursively for a .uproject.
    pub asset_name: Option<String>,
    /// Optional Fab identifiers to trigger a download prior to create (reusing the same download flow as import).
    pub namespace: Option<String>,
    pub asset_id: Option<String>,
    pub artifact_id: Option<String>,
    /// Optional Unreal Engine major.minor version (e.g., "5.6") to select engine and set EngineAssociation.
    pub ue: Option<String>,
    pub output_dir: String,
    pub project_name: String,
    pub project_type: Option<String>, // "bp" or "cpp"
    /// When true, launch Unreal Editor to open the created project after copying. Defaults to false.
    pub open_after_create: Option<bool>,
    pub dry_run: Option<bool>,
    /// Optional job id to stream progress over WebSocket
    pub job_id: Option<String>,
}

#[derive(Serialize)]
pub struct CreateUnrealProjectResponse {
    pub ok: bool,
    pub message: String,
    pub command: String,
    pub project_path: Option<String>,
}

// === WebSocket progress broadcasting ===
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProgressEvent {
    pub job_id: String,
    pub phase: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ===== Configuration: Paths for Projects and Engines =====
#[derive(Serialize, Deserialize)]
pub struct PathsStatus {
    pub configured: PathsConfig,
    pub effective_projects_dir: String,
    pub effective_engines_dir: String,
    pub effective_cache_dir: String,
    pub effective_downloads_dir: String,
}

#[derive(Deserialize)]
pub struct PathsUpdate {
    pub projects_dir: Option<String>,
    pub engines_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub downloads_dir: Option<String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct PathsConfig {
    pub projects_dir: Option<String>,
    pub engines_dir: Option<String>,
    pub cache_dir: Option<String>,
    pub downloads_dir: Option<String>,
}

#[derive(Serialize)]
pub struct UnrealProjectInfo {
    pub name: String,
    pub path: String,
    pub uproject_file: String,
    pub engine_version: String,
}

#[derive(Serialize)]
pub struct UnrealProjectsResponse {
    pub base_directory: String,
    pub projects: Vec<UnrealProjectInfo>,
}

#[derive(Serialize)]
pub struct UnrealEngineInfo {
    pub name: String,
    pub version: String,
    pub path: String,
    pub editor_path: Option<String>,
}

#[derive(Serialize)]
pub struct UnrealEnginesResponse {
    pub base_directory: String,
    pub engines: Vec<UnrealEngineInfo>,
}

#[derive(Default)]
pub struct Totals {
    pub downloaded: usize,
    pub skipped_zero: usize,
    pub up_to_date: usize
}

#[derive(Serialize, Deserialize)]
pub struct SetProjectEngineRequest {
    pub project: String, // project dir or .uproject path
    pub version: String, // e.g., "5.6" or "5.6.1" or "UE_5.6"
}

#[derive(Serialize)]
pub struct SimpleResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Deserialize)]
pub struct AuthCompleteRequest {
    pub code: String
}