use std::collections::HashMap;

use axum::{
    Extension, Router,
    extract::{Json as RequestJson, State},
    response::Json as ResponseJson,
    routing::post,
};
use db::models::{
    execution_process::{ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus},
    session::{CreateSession, Session},
    workspace::Workspace,
    workspace_repo::WorkspaceRepo,
};
use deployment::Deployment;
use executors::actions::{
    ExecutorAction, ExecutorActionType,
    script::{ScriptContext, ScriptRequest, ScriptRequestLanguage},
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use services::services::container::ContainerService;
use ts_rs::TS;
use utils::response::ApiResponse;
use uuid::Uuid;

use crate::{DeploymentImpl, error::ApiError};

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(tag = "type", rename_all = "snake_case")]
pub enum RunScriptError {
    NoScriptConfigured,
    ProcessAlreadyRunning,
}

#[derive(Debug, Deserialize, Default)]
pub struct StartDevServerRequest {
    #[serde(default)]
    pub repo_script_ids: HashMap<Uuid, String>,
}

#[derive(Debug)]
struct DevServerScriptEntry {
    id: String,
    script: String,
}

fn parse_repo_dev_server_scripts(raw: &str) -> Vec<DevServerScriptEntry> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let parsed = serde_json::from_str::<JsonValue>(trimmed);
    if let Ok(json) = parsed {
        if let Some(array) = json.as_array() {
            let scripts = array
                .iter()
                .enumerate()
                .filter_map(|(index, item)| {
                    let script = item.get("script")?.as_str()?.trim();
                    if script.is_empty() {
                        return None;
                    }
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .filter(|value| !value.trim().is_empty())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| format!("script-{}", index + 1));
                    Some(DevServerScriptEntry {
                        id,
                        script: script.to_string(),
                    })
                })
                .collect::<Vec<_>>();
            if !scripts.is_empty() {
                return scripts;
            }
        }

        if let Some(obj) = json.as_object() {
            let maybe_entries = obj.get("scripts").or_else(|| obj.get("entries"));
            if let Some(entries) = maybe_entries.and_then(|value| value.as_array()) {
                let scripts = entries
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        let script = item.get("script")?.as_str()?.trim();
                        if script.is_empty() {
                            return None;
                        }
                        let id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .filter(|value| !value.trim().is_empty())
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| format!("script-{}", index + 1));
                        Some(DevServerScriptEntry {
                            id,
                            script: script.to_string(),
                        })
                    })
                    .collect::<Vec<_>>();
                if !scripts.is_empty() {
                    return scripts;
                }
            }

            if let Some(script) = obj.get("script").and_then(|value| value.as_str()) {
                let script = script.trim();
                if !script.is_empty() {
                    return vec![DevServerScriptEntry {
                        id: "default".to_string(),
                        script: script.to_string(),
                    }];
                }
            }
        }
    }

    vec![DevServerScriptEntry {
        id: "default".to_string(),
        script: trimmed.to_string(),
    }]
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/dev-server/start", post(start_dev_server))
        .route("/cleanup", post(run_cleanup_script))
        .route("/archive", post(run_archive_script))
        .route("/stop", post(stop_workspace_execution))
}

#[axum::debug_handler]
pub async fn start_dev_server(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
    RequestJson(payload): RequestJson<Option<StartDevServerRequest>>,
) -> Result<ResponseJson<ApiResponse<Vec<ExecutionProcess>>>, ApiError> {
    let pool = &deployment.db().pool;

    let existing_dev_servers =
        match ExecutionProcess::find_running_dev_servers_by_workspace(pool, workspace.id).await {
            Ok(servers) => servers,
            Err(e) => {
                tracing::error!(
                    "Failed to find running dev servers for workspace {}: {}",
                    workspace.id,
                    e
                );
                return Err(ApiError::Workspace(
                    db::models::workspace::WorkspaceError::ValidationError(e.to_string()),
                ));
            }
        };

    for dev_server in existing_dev_servers {
        tracing::info!(
            "Stopping existing dev server {} for workspace {}",
            dev_server.id,
            workspace.id
        );

        if let Err(e) = deployment
            .container()
            .stop_execution(&dev_server, ExecutionProcessStatus::Killed)
            .await
        {
            tracing::error!("Failed to stop dev server {}: {}", dev_server.id, e);
        }
    }

    let repos = WorkspaceRepo::find_repos_for_workspace(pool, workspace.id).await?;
    let script_selections = payload.unwrap_or_default().repo_script_ids;

    let repos_with_dev_script: Vec<_> = repos
        .iter()
        .filter_map(|repo| {
            let raw = repo.dev_server_script.as_ref()?;
            let parsed = parse_repo_dev_server_scripts(raw);
            if parsed.is_empty() {
                return None;
            }

            let selected_id = script_selections.get(&repo.id);
            let selected_script = if let Some(script_id) = selected_id {
                parsed
                    .iter()
                    .find(|entry| &entry.id == script_id)
                    .map(|entry| entry.script.clone())
                    .or_else(|| parsed.first().map(|entry| entry.script.clone()))
            } else {
                parsed.first().map(|entry| entry.script.clone())
            };

            selected_script.map(|script| (repo, script))
        })
        .collect();

    if repos_with_dev_script.is_empty() {
        return Ok(ResponseJson(ApiResponse::error(
            "No dev server script configured for any repository in this workspace",
        )));
    }

    let session = match Session::find_latest_by_workspace_id(pool, workspace.id).await? {
        Some(s) => s,
        None => {
            Session::create(
                pool,
                &CreateSession {
                    executor: Some("dev-server".to_string()),
                    name: None,
                },
                Uuid::new_v4(),
                workspace.id,
            )
            .await?
        }
    };

    let mut execution_processes = Vec::new();
    for (repo, selected_script) in repos_with_dev_script {
        let executor_action = ExecutorAction::new(
            ExecutorActionType::ScriptRequest(ScriptRequest {
                script: selected_script,
                language: ScriptRequestLanguage::Bash,
                context: ScriptContext::DevServer,
                working_dir: Some(repo.name.clone()),
            }),
            None,
        );

        let execution_process = deployment
            .container()
            .start_execution(
                &workspace,
                &session,
                &executor_action,
                &ExecutionProcessRunReason::DevServer,
            )
            .await?;
        execution_processes.push(execution_process);
    }

    deployment
        .track_if_analytics_allowed(
            "dev_server_started",
            serde_json::json!({
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(execution_processes)))
}

pub async fn stop_workspace_execution(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<()>>, ApiError> {
    deployment.container().try_stop(&workspace, false).await;

    deployment
        .track_if_analytics_allowed(
            "task_attempt_stopped",
            serde_json::json!({
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(())))
}

#[axum::debug_handler]
pub async fn run_cleanup_script(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<ExecutionProcess, RunScriptError>>, ApiError> {
    let pool = &deployment.db().pool;

    if ExecutionProcess::has_running_non_dev_server_processes_for_workspace(pool, workspace.id)
        .await?
    {
        return Ok(ResponseJson(ApiResponse::error_with_data(
            RunScriptError::ProcessAlreadyRunning,
        )));
    }

    deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;

    let repos = WorkspaceRepo::find_repos_for_workspace(pool, workspace.id).await?;
    let executor_action = match deployment.container().cleanup_actions_for_repos(&repos) {
        Some(action) => action,
        None => {
            return Ok(ResponseJson(ApiResponse::error_with_data(
                RunScriptError::NoScriptConfigured,
            )));
        }
    };

    let session = match Session::find_latest_by_workspace_id(pool, workspace.id).await? {
        Some(s) => s,
        None => {
            Session::create(
                pool,
                &CreateSession {
                    executor: None,
                    name: None,
                },
                Uuid::new_v4(),
                workspace.id,
            )
            .await?
        }
    };

    let execution_process = deployment
        .container()
        .start_execution(
            &workspace,
            &session,
            &executor_action,
            &ExecutionProcessRunReason::CleanupScript,
        )
        .await?;

    deployment
        .track_if_analytics_allowed(
            "cleanup_script_executed",
            serde_json::json!({
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(execution_process)))
}

pub async fn run_archive_script(
    Extension(workspace): Extension<Workspace>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<ExecutionProcess, RunScriptError>>, ApiError> {
    let pool = &deployment.db().pool;
    if ExecutionProcess::has_running_non_dev_server_processes_for_workspace(pool, workspace.id)
        .await?
    {
        return Ok(ResponseJson(ApiResponse::error_with_data(
            RunScriptError::ProcessAlreadyRunning,
        )));
    }

    deployment
        .container()
        .ensure_container_exists(&workspace)
        .await?;

    let repos = WorkspaceRepo::find_repos_for_workspace(pool, workspace.id).await?;
    let executor_action = match deployment.container().archive_actions_for_repos(&repos) {
        Some(action) => action,
        None => {
            return Ok(ResponseJson(ApiResponse::error_with_data(
                RunScriptError::NoScriptConfigured,
            )));
        }
    };
    let session = match Session::find_latest_by_workspace_id(pool, workspace.id).await? {
        Some(s) => s,
        None => {
            Session::create(
                pool,
                &CreateSession {
                    executor: None,
                    name: None,
                },
                Uuid::new_v4(),
                workspace.id,
            )
            .await?
        }
    };

    let execution_process = deployment
        .container()
        .start_execution(
            &workspace,
            &session,
            &executor_action,
            &ExecutionProcessRunReason::ArchiveScript,
        )
        .await?;

    deployment
        .track_if_analytics_allowed(
            "archive_script_executed",
            serde_json::json!({
                "workspace_id": workspace.id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(execution_process)))
}
