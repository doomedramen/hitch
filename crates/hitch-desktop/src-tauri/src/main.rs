#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod repo;
mod types;

use anyhow::Result;
use hitch::commands::global_context::GlobalContext;
use hitch::utils::logging::Logger;
use hitch::utils::output::{BufferedOutputSink, OutputSink};
use std::sync::Arc;

use crate::types::{
    BranchDetailsDto, EnvironmentDetailsDto, OperationResultDto, RepoProbeResultDto,
    WorkspaceIndexDto,
};

fn context_at(repo_path: &str) -> Result<GlobalContext> {
    let logger = Arc::new(Logger::for_command("desktop", false));
    GlobalContext::new_at_path(repo_path, false, false, logger)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

#[tauri::command]
async fn repo_probe(path: String) -> RepoProbeResultDto {
    let path_for_err = path.clone();
    tauri::async_runtime::spawn_blocking(move || repo::probe_repo(&path))
        .await
        .unwrap_or_else(|e| RepoProbeResultDto::err(path_for_err, format!("Internal error: {}", e)))
}

#[tauri::command]
async fn workspace_index(repo_path: String) -> Result<WorkspaceIndexDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context_at(&repo_path).map_err(|e| e.to_string())?;
        let model = hitch::core::workspace_index::build_workspace_index_model(&ctx)
            .map_err(|e| e.to_string())?;
        Ok(WorkspaceIndexDto::from(model))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn branch_details(
    repo_path: String,
    branch_name: String,
) -> Result<BranchDetailsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context_at(&repo_path).map_err(|e| e.to_string())?;

        // Find the branch row from a fresh index (keeps the API simple for the UI).
        let index = hitch::core::workspace_index::build_workspace_index_model(&ctx)
            .map_err(|e| e.to_string())?;
        let row = index
            .promoted_branches
            .iter()
            .chain(index.branches.iter())
            .find(|b| b.name == branch_name)
            .cloned()
            .ok_or_else(|| format!("Branch '{}' not found", branch_name))?;

        let metadata_sha = ctx.git().get_branch_commit_sha("hitch-metadata").ok();
        let branch_sha = ctx
            .git()
            .get_branch_commit_sha(&row.name)
            .map_err(|e| e.to_string())?;

        let model =
            hitch::core::details::build_branch_details_model(&ctx, &row, branch_sha, metadata_sha)
                .map_err(|e| e.to_string())?;
        Ok(BranchDetailsDto::from(model))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn env_details(repo_path: String, env_name: String) -> Result<EnvironmentDetailsDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context_at(&repo_path).map_err(|e| e.to_string())?;
        let metadata_sha = ctx.git().get_branch_commit_sha("hitch-metadata").ok();
        let env_sha = ctx
            .git()
            .get_branch_commit_sha(&env_name)
            .map_err(|e| e.to_string())?;

        let model = hitch::core::details::build_environment_details_model(
            &ctx,
            &env_name,
            env_sha,
            metadata_sha,
        )
        .map_err(|e| e.to_string())?;
        Ok(EnvironmentDetailsDto::from(model))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn op_context_with_sink(repo_path: &str) -> Result<(GlobalContext, Arc<BufferedOutputSink>)> {
    let sink = BufferedOutputSink::new();
    let ctx = context_at(repo_path)?.with_output(sink.clone() as Arc<dyn OutputSink>);
    Ok((ctx, sink))
}

#[tauri::command]
async fn promote(repo_path: String, branch_name: String, env_name: String) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        let (ctx, sink) = match op_context_with_sink(&repo_path) {
            Ok(v) => v,
            Err(e) => return OperationResultDto::error(e.to_string(), vec![]),
        };

        let args = hitch::commands::promote::PromoteCommand {
            branch: branch_name,
            env_name,
            no_rebuild: false,
        };

        let res = hitch::commands::promote::run(args, &ctx);
        OperationResultDto::from_result(res.map_err(|e| e.to_string()), sink.snapshot())
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

#[tauri::command]
async fn rebuild(repo_path: String, env_name: String, force: bool) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        let (ctx, sink) = match op_context_with_sink(&repo_path) {
            Ok(v) => v,
            Err(e) => return OperationResultDto::error(e.to_string(), vec![]),
        };

        let args = hitch::commands::rebuild::RebuildCommand { env_name, force };
        let res = hitch::commands::rebuild::run(args, &ctx);
        OperationResultDto::from_result(res.map_err(|e| e.to_string()), sink.snapshot())
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

#[tauri::command]
async fn release(repo_path: String, env_name: String) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        let (ctx, sink) = match op_context_with_sink(&repo_path) {
            Ok(v) => v,
            Err(e) => return OperationResultDto::error(e.to_string(), vec![]),
        };

        // The desktop app provides confirmation via UI, so always run with force=true.
        let args = hitch::commands::release::ReleaseCommand {
            env_name,
            target_branch: None,
            force: true,
            no_prune: false,
            no_rebuild_dependents: false,
            squash: false,
        };

        let res = hitch::commands::release::run(args, &ctx);
        OperationResultDto::from_result(res.map_err(|e| e.to_string()), sink.snapshot())
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![
            repo_probe,
            workspace_index,
            branch_details,
            env_details,
            promote,
            rebuild,
            release
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
