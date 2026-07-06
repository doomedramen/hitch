#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod repo;
mod types;

use anyhow::Result;
use hitch::commands::global_context::GlobalContext;
use hitch::utils::logging::Logger;
use hitch::utils::output::{BufferedOutputSink, OutputSink};
use std::sync::Arc;

use crate::types::{
    ApprovalRequestDto, ApprovalsListDto, BranchDetailsDto, EnvironmentDetailsDto,
    OperationResultDto, RepoProbeResultDto, WorkspaceIndexDto,
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

        // Serialize against any other Hitch operation on the same repo (CLI or app).
        let _repo_lock = match hitch::utils::repo_lock::RepoLock::acquire(
            std::path::Path::new(&ctx.git().get_git_dir()),
            "promote",
        ) {
            Ok(lock) => lock,
            Err(e) => return OperationResultDto::error(e.to_string(), sink.snapshot()),
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

        // Serialize against any other Hitch operation on the same repo (CLI or app).
        let _repo_lock = match hitch::utils::repo_lock::RepoLock::acquire(
            std::path::Path::new(&ctx.git().get_git_dir()),
            "rebuild",
        ) {
            Ok(lock) => lock,
            Err(e) => return OperationResultDto::error(e.to_string(), sink.snapshot()),
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

        // Serialize against any other Hitch operation on the same repo (CLI or app).
        let _repo_lock = match hitch::utils::repo_lock::RepoLock::acquire(
            std::path::Path::new(&ctx.git().get_git_dir()),
            "release",
        ) {
            Ok(lock) => lock,
            Err(e) => return OperationResultDto::error(e.to_string(), sink.snapshot()),
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

#[tauri::command]
async fn approvals_list(repo_path: String) -> Result<ApprovalsListDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ctx = context_at(&repo_path).map_err(|e| e.to_string())?;

        // All requests, newest-first (the UI groups/filters by status).
        let requests = hitch::utils::prelude::get_approval_requests(&ctx, None, None)
            .map_err(|e| e.to_string())?;
        let config = hitch::utils::prelude::access_metadata_read_only(&ctx, |c| Ok(c.clone()))
            .map_err(|e| e.to_string())?;
        // A missing git user email is not fatal — the viewer_* hints just resolve
        // to false, so the UI shows the requests read-only.
        let current_user = hitch::utils::authorization::get_current_user(&ctx).ok();

        let requests = requests
            .iter()
            .map(|r| ApprovalRequestDto::build(&ctx, &config, r, current_user.as_deref()))
            .collect();

        Ok(ApprovalsListDto {
            current_user,
            requests,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Run a mutating approval action under the repository lock, capturing its output.
///
/// Mirrors the promote/rebuild/release pattern: the desktop takes the repo-wide
/// lock (the command's own `with_locked_env`/`modify_metadata` do the rest) and
/// surfaces the buffered log lines to the UI regardless of success or failure.
fn run_locked_approval<F>(repo_path: &str, lock_label: &str, action: F) -> OperationResultDto
where
    F: FnOnce(&GlobalContext) -> Result<()>,
{
    let (ctx, sink) = match op_context_with_sink(repo_path) {
        Ok(v) => v,
        Err(e) => return OperationResultDto::error(e.to_string(), vec![]),
    };

    // Serialize against any other Hitch operation on the same repo (CLI or app).
    let _repo_lock = match hitch::utils::repo_lock::RepoLock::acquire(
        std::path::Path::new(&ctx.git().get_git_dir()),
        lock_label,
    ) {
        Ok(lock) => lock,
        Err(e) => return OperationResultDto::error(e.to_string(), sink.snapshot()),
    };

    let res = action(&ctx);
    OperationResultDto::from_result(res.map_err(|e| e.to_string()), sink.snapshot())
}

#[tauri::command]
async fn approval_approve(
    repo_path: String,
    request_id: String,
    comment: Option<String>,
) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        run_locked_approval(&repo_path, "approvals approve", |ctx| {
            let args = hitch::commands::approvals::approve::ApproveArgs {
                request_id,
                // Normalize an empty/whitespace comment to None.
                comment: comment.filter(|c| !c.trim().is_empty()),
            };
            hitch::commands::approvals::approve::run(args, ctx)
        })
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

#[tauri::command]
async fn approval_reject(
    repo_path: String,
    request_id: String,
    reason: String,
) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        run_locked_approval(&repo_path, "approvals reject", |ctx| {
            let args = hitch::commands::approvals::reject::RejectArgs { request_id, reason };
            hitch::commands::approvals::reject::run(args, ctx)
        })
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

#[tauri::command]
async fn approval_cancel(repo_path: String, request_id: String) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        run_locked_approval(&repo_path, "approvals cancel", |ctx| {
            // force=true: the desktop confirms via UI, and there is no stdin to
            // read an interactive confirmation from.
            let args = hitch::commands::approvals::cancel::CancelArgs {
                request_id,
                force: true,
            };
            hitch::commands::approvals::cancel::run(args, ctx)
        })
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

#[tauri::command]
async fn approval_refresh(repo_path: String, request_id: String) -> OperationResultDto {
    tauri::async_runtime::spawn_blocking(move || {
        run_locked_approval(&repo_path, "approvals refresh", |ctx| {
            let args = hitch::commands::approvals::refresh::RefreshArgs { request_id };
            hitch::commands::approvals::refresh::run(args, ctx)
        })
    })
    .await
    .unwrap_or_else(|e| OperationResultDto::error(format!("Internal error: {}", e), vec![]))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            // Apply native macOS vibrancy (NSVisualEffectView) behind the window.
            // The content pane stays opaque via CSS; only the translucent sidebar
            // and toolbar let this material show through.
            #[cfg(target_os = "macos")]
            {
                use tauri::Manager;
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                if let Some(window) = app.get_webview_window("main") {
                    let _ = apply_vibrancy(
                        &window,
                        NSVisualEffectMaterial::Sidebar,
                        None,
                        None,
                    );
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            repo_probe,
            workspace_index,
            branch_details,
            env_details,
            promote,
            rebuild,
            release,
            approvals_list,
            approval_approve,
            approval_reject,
            approval_cancel,
            approval_refresh
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
