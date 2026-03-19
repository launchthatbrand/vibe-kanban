use std::{
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    response::Json as ResponseJson,
    routing::{get, put},
};
use db::models::{workspace::Workspace, workspace_repo::WorkspaceRepo};
use deployment::Deployment;
use serde::{Deserialize, Serialize};
use services::services::container::ContainerService;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

const MAX_TEXT_FILE_BYTES: u64 = 1_048_576;
const MAX_TREE_ENTRIES: usize = 50_000;

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileTreeQuery {
    pub repo_id: Option<Uuid>,
    pub path: Option<String>,
    pub recursive: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileContentQuery {
    pub repo_id: Option<Uuid>,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileTreeEntry {
    pub name: String,
    pub path: String,
    pub is_directory: bool,
    pub size_bytes: Option<u64>,
    pub modified_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileTreeResponse {
    pub entries: Vec<WorkspaceFileTreeEntry>,
    pub current_path: String,
    pub repo_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileContentResponse {
    pub path: String,
    pub content: Option<String>,
    pub is_binary: bool,
    pub is_too_large: bool,
    pub size_bytes: u64,
    pub modified_at_ms: Option<u64>,
    pub encoding: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileContentUpdateRequest {
    pub repo_id: Option<Uuid>,
    pub path: String,
    pub content: String,
    pub expected_modified_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, TS)]
pub struct WorkspaceFileContentUpdateResponse {
    pub path: String,
    pub size_bytes: u64,
    pub modified_at_ms: Option<u64>,
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/tree", get(get_workspace_file_tree))
        .route("/content", get(get_workspace_file_content).put(update_workspace_file_content))
}

pub async fn get_workspace_file_tree(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<WorkspaceFileTreeQuery>,
) -> Result<ResponseJson<ApiResponse<WorkspaceFileTreeResponse>>, ApiError> {
    let base_dir = resolve_workspace_base_dir(&deployment, &workspace, query.repo_id).await?;
    let base_dir = tokio::fs::canonicalize(&base_dir).await?;

    let relative_path = validate_relative_path(query.path.as_deref().unwrap_or(""))?;
    let current_dir = if relative_path.as_os_str().is_empty() {
        base_dir.clone()
    } else {
        base_dir.join(&relative_path)
    };
    let current_dir = tokio::fs::canonicalize(&current_dir).await?;
    ensure_path_within_base(&base_dir, &current_dir)?;

    let recursive = query.recursive.unwrap_or(true);
    let entries = collect_tree_entries(&base_dir, &current_dir, recursive).await?;
    let current_path = path_to_unix_string(&relative_path);

    Ok(ResponseJson(ApiResponse::success(WorkspaceFileTreeResponse {
        entries,
        current_path,
        repo_id: query.repo_id,
    })))
}

pub async fn get_workspace_file_content(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Query(query): Query<WorkspaceFileContentQuery>,
) -> Result<ResponseJson<ApiResponse<WorkspaceFileContentResponse>>, ApiError> {
    let base_dir = resolve_workspace_base_dir(&deployment, &workspace, query.repo_id).await?;
    let base_dir = tokio::fs::canonicalize(&base_dir).await?;
    let relative_path = validate_relative_path(&query.path)?;
    if relative_path.as_os_str().is_empty() {
        return Err(ApiError::BadRequest("File path is required".to_string()));
    }

    let full_path = base_dir.join(&relative_path);
    let canonical_path = tokio::fs::canonicalize(&full_path).await?;
    ensure_path_within_base(&base_dir, &canonical_path)?;

    let metadata = tokio::fs::metadata(&canonical_path).await?;
    if !metadata.is_file() {
        return Err(ApiError::BadRequest("Path is not a file".to_string()));
    }

    let size_bytes = metadata.len();
    let modified_at_ms = metadata.modified().ok().and_then(system_time_to_ms);

    if size_bytes > MAX_TEXT_FILE_BYTES {
        return Ok(ResponseJson(ApiResponse::success(WorkspaceFileContentResponse {
            path: path_to_unix_string(&relative_path),
            content: None,
            is_binary: false,
            is_too_large: true,
            size_bytes,
            modified_at_ms,
            encoding: Some("utf-8".to_string()),
        })));
    }

    let bytes = tokio::fs::read(&canonical_path).await?;
    if is_binary_data(&bytes) {
        return Ok(ResponseJson(ApiResponse::success(WorkspaceFileContentResponse {
            path: path_to_unix_string(&relative_path),
            content: None,
            is_binary: true,
            is_too_large: false,
            size_bytes,
            modified_at_ms,
            encoding: None,
        })));
    }

    let content = String::from_utf8(bytes)
        .map_err(|_| ApiError::BadRequest("File is not valid UTF-8 text".to_string()))?;

    Ok(ResponseJson(ApiResponse::success(WorkspaceFileContentResponse {
        path: path_to_unix_string(&relative_path),
        content: Some(content),
        is_binary: false,
        is_too_large: false,
        size_bytes,
        modified_at_ms,
        encoding: Some("utf-8".to_string()),
    })))
}

pub async fn update_workspace_file_content(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<WorkspaceFileContentUpdateRequest>,
) -> Result<ResponseJson<ApiResponse<WorkspaceFileContentUpdateResponse>>, ApiError> {
    let base_dir = resolve_workspace_base_dir(&deployment, &workspace, payload.repo_id).await?;
    let base_dir = tokio::fs::canonicalize(&base_dir).await?;
    let relative_path = validate_relative_path(&payload.path)?;
    if relative_path.as_os_str().is_empty() {
        return Err(ApiError::BadRequest("File path is required".to_string()));
    }

    let full_path = base_dir.join(&relative_path);
    let canonical_path = if tokio::fs::metadata(&full_path).await.is_ok() {
        tokio::fs::canonicalize(&full_path).await?
    } else {
        let parent = full_path
            .parent()
            .ok_or_else(|| ApiError::BadRequest("Invalid file path".to_string()))?;
        let canonical_parent = tokio::fs::canonicalize(parent).await?;
        ensure_path_within_base(&base_dir, &canonical_parent)?;
        full_path
    };

    ensure_path_within_base(&base_dir, &canonical_path)?;

    if let Some(expected_modified_at_ms) = payload.expected_modified_at_ms
        && let Ok(metadata) = tokio::fs::metadata(&canonical_path).await
        && let Some(current_modified_at_ms) = metadata.modified().ok().and_then(system_time_to_ms)
        && current_modified_at_ms != expected_modified_at_ms
    {
        return Err(ApiError::Conflict(
            "File has changed on disk. Reload and retry save.".to_string(),
        ));
    }

    tokio::fs::write(&canonical_path, payload.content.as_bytes()).await?;
    let metadata = tokio::fs::metadata(&canonical_path).await?;

    Ok(ResponseJson(ApiResponse::success(
        WorkspaceFileContentUpdateResponse {
            path: path_to_unix_string(&relative_path),
            size_bytes: metadata.len(),
            modified_at_ms: metadata.modified().ok().and_then(system_time_to_ms),
        },
    )))
}

async fn resolve_workspace_base_dir(
    deployment: &DeploymentImpl,
    workspace: &Workspace,
    repo_id: Option<Uuid>,
) -> Result<PathBuf, ApiError> {
    let container_ref = deployment
        .container()
        .ensure_container_exists(workspace)
        .await?;
    let workspace_root = PathBuf::from(container_ref);
    let repos = WorkspaceRepo::find_repos_for_workspace(&deployment.db().pool, workspace.id).await?;

    if repos.is_empty() {
        return Ok(workspace_root);
    }

    let selected_repo = match repo_id {
        Some(repo_id) => repos
            .iter()
            .find(|repo| repo.id == repo_id)
            .ok_or_else(|| ApiError::BadRequest("Repository not found in workspace".to_string()))?,
        None if repos.len() == 1 => &repos[0],
        None => {
            return Err(ApiError::BadRequest(
                "repo_id is required for workspaces with multiple repositories".to_string(),
            ));
        }
    };

    Ok(workspace_root.join(&selected_repo.name))
}

async fn collect_tree_entries(
    base_dir: &Path,
    current_dir: &Path,
    recursive: bool,
) -> Result<Vec<WorkspaceFileTreeEntry>, ApiError> {
    let mut entries = Vec::new();
    let mut dirs = vec![current_dir.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let mut read_dir = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = read_dir.next_entry().await? {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name == ".git" {
                continue;
            }

            let path = entry.path();
            let metadata = entry.metadata().await?;
            let is_directory = metadata.is_dir();
            let relative_path = path
                .strip_prefix(base_dir)
                .map_err(|_| ApiError::BadRequest("Invalid workspace path".to_string()))?;
            let relative_path = path_to_unix_string(relative_path);

            entries.push(WorkspaceFileTreeEntry {
                name: file_name,
                path: relative_path.clone(),
                is_directory,
                size_bytes: if is_directory { None } else { Some(metadata.len()) },
                modified_at_ms: metadata.modified().ok().and_then(system_time_to_ms),
            });

            if recursive && is_directory {
                dirs.push(path);
            }

            if entries.len() > MAX_TREE_ENTRIES {
                return Err(ApiError::BadRequest(format!(
                    "File tree is too large (>{MAX_TREE_ENTRIES} entries). Narrow the path."
                )));
            }
        }
    }

    entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.path.cmp(&b.path),
    });

    Ok(entries)
}

fn ensure_path_within_base(base_dir: &Path, target_path: &Path) -> Result<(), ApiError> {
    if target_path.starts_with(base_dir) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "Path is outside of workspace boundary".to_string(),
        ))
    }
}

fn validate_relative_path(raw_path: &str) -> Result<PathBuf, ApiError> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    let path = PathBuf::from(trimmed);
    if path.is_absolute() {
        return Err(ApiError::BadRequest(
            "Absolute paths are not allowed".to_string(),
        ));
    }

    for component in path.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ApiError::BadRequest(
                    "Path traversal is not allowed".to_string(),
                ));
            }
            _ => {}
        }
    }

    Ok(path)
}

fn path_to_unix_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn system_time_to_ms(time: std::time::SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn is_binary_data(data: &[u8]) -> bool {
    data.iter().take(8_192).any(|byte| *byte == 0)
}
