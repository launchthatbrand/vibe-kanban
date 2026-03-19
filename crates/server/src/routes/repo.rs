use std::path::{Component, PathBuf};

use axum::{
    Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json as ResponseJson,
    routing::{get, post},
};
use db::models::repo::{Repo, SearchResult, UpdateRepo};
use deployment::Deployment;
use git::{GitBranch, GitRemote};
use git_host::{GitHostError, GitHostProvider, GitHostService, OpenPrInfo, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use services::services::file_search::SearchQuery;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(serde::Deserialize)]
pub struct OpenEditorRequest {
    pub editor_type: Option<String>,
    pub git_repo_path: Option<PathBuf>,
}

#[derive(Debug, serde::Serialize, ts_rs::TS)]
pub struct OpenEditorResponse {
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
pub struct RegisterRepoRequest {
    pub path: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
pub struct InitRepoRequest {
    pub parent_path: String,
    pub folder_name: String,
}

#[derive(Debug, Deserialize, TS)]
pub struct BatchRepoRequest {
    pub ids: Vec<Uuid>,
}

#[derive(Debug, Serialize, TS)]
pub struct MonorepoApp {
    pub id: String,
    pub name: String,
    pub relative_path: String,
    pub script: String,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverMonorepoAppsQuery {
    pub apps_root: Option<String>,
}

fn validate_relative_apps_root(input: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(input.trim());
    if candidate.as_os_str().is_empty() || candidate.is_absolute() {
        return None;
    }
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return None;
    }
    Some(candidate)
}

pub async fn discover_monorepo_apps(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
    Query(query): Query<DiscoverMonorepoAppsQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<MonorepoApp>>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;

    let configured_apps_root = query
        .apps_root
        .as_deref()
        .or(repo.apps_root.as_deref())
        .unwrap_or("apps");
    let relative_apps_root = validate_relative_apps_root(configured_apps_root)
        .unwrap_or_else(|| PathBuf::from("apps"));
    let apps_dir = repo.path.join(&relative_apps_root);
    if !apps_dir.exists() || !apps_dir.is_dir() {
        return Ok(ResponseJson(ApiResponse::success(Vec::new())));
    }

    let mut apps = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&apps_dir) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let app_dir = entry.path();
            let package_json_path = app_dir.join("package.json");
            if !package_json_path.exists() {
                continue;
            }

            let Ok(raw_package_json) = std::fs::read_to_string(&package_json_path) else {
                continue;
            };
            let Ok(parsed_package_json) = serde_json::from_str::<JsonValue>(&raw_package_json)
            else {
                continue;
            };

            let has_dev_script = parsed_package_json
                .get("scripts")
                .and_then(|scripts| scripts.get("dev"))
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if !has_dev_script {
                continue;
            }

            let fallback_name = entry.file_name().to_string_lossy().to_string();
            let app_name = parsed_package_json
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or(fallback_name);

            let relative_path = relative_apps_root.join(entry.file_name());
            let relative_path_string = relative_path.to_string_lossy().to_string();

            apps.push(MonorepoApp {
                id: app_name
                    .to_lowercase()
                    .replace(|c: char| !c.is_ascii_alphanumeric(), "-"),
                name: app_name,
                relative_path: relative_path_string.clone(),
                script: format!("cd {} && pnpm dev", relative_path_string),
            });
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(ResponseJson(ApiResponse::success(apps)))
}

pub async fn register_repo(
    State(deployment): State<DeploymentImpl>,
    ResponseJson(payload): ResponseJson<RegisterRepoRequest>,
) -> Result<ResponseJson<ApiResponse<Repo>>, ApiError> {
    let repo = deployment
        .repo()
        .register(
            &deployment.db().pool,
            &payload.path,
            payload.display_name.as_deref(),
        )
        .await?;

    Ok(ResponseJson(ApiResponse::success(repo)))
}

pub async fn init_repo(
    State(deployment): State<DeploymentImpl>,
    ResponseJson(payload): ResponseJson<InitRepoRequest>,
) -> Result<ResponseJson<ApiResponse<Repo>>, ApiError> {
    let repo = deployment
        .repo()
        .init_repo(
            &deployment.db().pool,
            deployment.git(),
            &payload.parent_path,
            &payload.folder_name,
        )
        .await?;

    Ok(ResponseJson(ApiResponse::success(repo)))
}

pub async fn get_repo_branches(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Vec<GitBranch>>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;

    let branches = deployment.git().get_all_branches(&repo.path)?;
    Ok(ResponseJson(ApiResponse::success(branches)))
}

pub async fn get_repo_remotes(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Vec<GitRemote>>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;

    let remotes = deployment.git().list_remotes(&repo.path)?;
    Ok(ResponseJson(ApiResponse::success(remotes)))
}

pub async fn get_repos_batch(
    State(deployment): State<DeploymentImpl>,
    ResponseJson(payload): ResponseJson<BatchRepoRequest>,
) -> Result<ResponseJson<ApiResponse<Vec<Repo>>>, ApiError> {
    let repos = Repo::find_by_ids(&deployment.db().pool, &payload.ids).await?;
    Ok(ResponseJson(ApiResponse::success(repos)))
}

pub async fn get_repos(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<Repo>>>, ApiError> {
    let repos = Repo::list_all(&deployment.db().pool).await?;
    Ok(ResponseJson(ApiResponse::success(repos)))
}

pub async fn get_recent_repos(
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<Vec<Repo>>>, ApiError> {
    let repos = Repo::list_by_recent_workspace_usage(&deployment.db().pool).await?;
    Ok(ResponseJson(ApiResponse::success(repos)))
}

pub async fn get_repo(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
) -> Result<ResponseJson<ApiResponse<Repo>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;
    Ok(ResponseJson(ApiResponse::success(repo)))
}

pub async fn update_repo(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
    ResponseJson(payload): ResponseJson<UpdateRepo>,
) -> Result<ResponseJson<ApiResponse<Repo>>, ApiError> {
    let repo = Repo::update(&deployment.db().pool, repo_id, &payload).await?;
    Ok(ResponseJson(ApiResponse::success(repo)))
}

pub async fn open_repo_in_editor(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
    ResponseJson(payload): ResponseJson<Option<OpenEditorRequest>>,
) -> Result<ResponseJson<ApiResponse<OpenEditorResponse>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;

    let editor_config = {
        let config = deployment.config().read().await;
        let editor_type_str = payload.as_ref().and_then(|req| req.editor_type.as_deref());
        config.editor.with_override(editor_type_str)
    };

    match editor_config.open_file(&repo.path).await {
        Ok(url) => {
            tracing::info!(
                "Opened editor for repo {} at path: {}{}",
                repo_id,
                repo.path.to_string_lossy(),
                if url.is_some() { " (remote mode)" } else { "" }
            );

            deployment
                .track_if_analytics_allowed(
                    "repo_editor_opened",
                    serde_json::json!({
                        "repo_id": repo_id.to_string(),
                        "editor_type": payload.as_ref().and_then(|req| req.editor_type.as_ref()),
                        "remote_mode": url.is_some(),
                    }),
                )
                .await;

            Ok(ResponseJson(ApiResponse::success(OpenEditorResponse {
                url,
            })))
        }
        Err(e) => {
            tracing::error!("Failed to open editor for repo {}: {:?}", repo_id, e);
            Err(ApiError::EditorOpen(e))
        }
    }
}

pub async fn search_repo(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
    Query(search_query): Query<SearchQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<SearchResult>>>, StatusCode> {
    if search_query.q.trim().is_empty() {
        return Ok(ResponseJson(ApiResponse::error(
            "Query parameter 'q' is required and cannot be empty",
        )));
    }

    let repo = match deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await
    {
        Ok(repo) => repo,
        Err(e) => {
            tracing::error!("Failed to get repo {}: {}", repo_id, e);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    match deployment
        .file_search_cache()
        .search_repo(&repo.path, &search_query.q, search_query.mode)
        .await
    {
        Ok(results) => Ok(ResponseJson(ApiResponse::success(results))),
        Err(e) => {
            tracing::error!("Failed to search files in repo {}: {}", repo_id, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum ListPrsError {
    CliNotInstalled { provider: ProviderKind },
    AuthFailed { message: String },
    UnsupportedProvider,
}

#[derive(Debug, Deserialize)]
pub struct ListPrsQuery {
    pub remote: Option<String>,
}

pub async fn list_open_prs(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
    Query(query): Query<ListPrsQuery>,
) -> Result<ResponseJson<ApiResponse<Vec<OpenPrInfo>, ListPrsError>>, ApiError> {
    let repo = deployment
        .repo()
        .get_by_id(&deployment.db().pool, repo_id)
        .await?;

    let remote = match query.remote {
        Some(name) => GitRemote {
            url: deployment.git().get_remote_url(&repo.path, &name)?,
            name,
        },
        None => deployment.git().get_default_remote(&repo.path)?,
    };

    let git_host = match GitHostService::from_url(&remote.url) {
        Ok(host) => host,
        Err(GitHostError::UnsupportedProvider) => {
            return Ok(ResponseJson(ApiResponse::error_with_data(
                ListPrsError::UnsupportedProvider,
            )));
        }
        Err(e) => {
            tracing::error!("Failed to create git host service: {}", e);
            return Ok(ResponseJson(ApiResponse::error(&e.to_string())));
        }
    };

    match git_host.list_open_prs(&repo.path, &remote.url).await {
        Ok(prs) => Ok(ResponseJson(ApiResponse::success(prs))),
        Err(GitHostError::CliNotInstalled { provider }) => Ok(ResponseJson(
            ApiResponse::error_with_data(ListPrsError::CliNotInstalled { provider }),
        )),
        Err(GitHostError::AuthFailed(message)) => Ok(ResponseJson(ApiResponse::error_with_data(
            ListPrsError::AuthFailed { message },
        ))),
        Err(GitHostError::UnsupportedProvider) => Ok(ResponseJson(ApiResponse::error_with_data(
            ListPrsError::UnsupportedProvider,
        ))),
        Err(e) => {
            tracing::error!("Failed to list open PRs for repo {}: {}", repo_id, e);
            Ok(ResponseJson(ApiResponse::error(&e.to_string())))
        }
    }
}

#[derive(Debug, Serialize, TS)]
pub struct DeleteRepoConflict {
    pub message: String,
    pub workspaces: Vec<String>,
}

pub async fn delete_repo(
    State(deployment): State<DeploymentImpl>,
    Path(repo_id): Path<Uuid>,
) -> Result<
    (
        StatusCode,
        ResponseJson<ApiResponse<(), DeleteRepoConflict>>,
    ),
    ApiError,
> {
    let active = Repo::active_workspace_names(&deployment.db().pool, repo_id).await?;
    if !active.is_empty() {
        return Ok((
            StatusCode::CONFLICT,
            ResponseJson(ApiResponse::error_with_data(DeleteRepoConflict {
                message: format!("Repository is used by {} active workspace(s)", active.len()),
                workspaces: active,
            })),
        ));
    }

    Repo::delete(&deployment.db().pool, repo_id).await?;
    Ok((StatusCode::OK, ResponseJson(ApiResponse::success(()))))
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/repos", get(get_repos).post(register_repo))
        .route("/repos/recent", get(get_recent_repos))
        .route("/repos/init", post(init_repo))
        .route("/repos/batch", post(get_repos_batch))
        .route(
            "/repos/{repo_id}",
            get(get_repo).put(update_repo).delete(delete_repo),
        )
        .route("/repos/{repo_id}/branches", get(get_repo_branches))
        .route("/repos/{repo_id}/remotes", get(get_repo_remotes))
        .route("/repos/{repo_id}/monorepo/apps", get(discover_monorepo_apps))
        .route("/repos/{repo_id}/prs", get(list_open_prs))
        .route("/repos/{repo_id}/search", get(search_repo))
        .route("/repos/{repo_id}/open-editor", post(open_repo_in_editor))
}
