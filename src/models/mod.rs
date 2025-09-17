use serde::{Deserialize, Serialize};

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
    pub asset_name: String,
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

#[derive(Serialize, Deserialize)]
pub struct CreateUnrealProjectRequest {
    pub engine_path: Option<String>,
    /// Path to a template/sample .uproject OR a directory containing one. If omitted, provide asset_name.
    pub template_project: Option<String>,
    /// Convenience: name of a downloaded asset under downloads/ (e.g., "Stack O Bot").
    /// When provided and template_project is empty, the server will search downloads/<asset_name>/ recursively for a .uproject.
    pub asset_name: Option<String>,
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