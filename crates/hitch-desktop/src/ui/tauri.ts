import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  ApprovalsList,
  BranchDetailsModel,
  BufferedLine,
  EnvironmentDetailsModel,
  OperationResult,
  RepoProbeResult,
  WorkspaceIndexModel
} from "./types";

/** A channel that streams operation output lines to the UI as they happen. */
export type LogChannel = Channel<BufferedLine>;

export function repoProbe(path: string): Promise<RepoProbeResult> {
  return invoke("repo_probe", { path });
}

export function workspaceIndex(repoPath: string): Promise<WorkspaceIndexModel> {
  return invoke("workspace_index", { repoPath });
}

export function branchDetails(repoPath: string, branchName: string): Promise<BranchDetailsModel> {
  return invoke("branch_details", { repoPath, branchName });
}

export function envDetails(repoPath: string, envName: string): Promise<EnvironmentDetailsModel> {
  return invoke("env_details", { repoPath, envName });
}

export function promote(
  repoPath: string,
  branchName: string,
  envName: string,
  onLog: LogChannel
): Promise<OperationResult> {
  return invoke("promote", { repoPath, branchName, envName, onLog });
}

export function rebuild(
  repoPath: string,
  envName: string,
  force: boolean,
  onLog: LogChannel
): Promise<OperationResult> {
  return invoke("rebuild", { repoPath, envName, force, onLog });
}

export function release(repoPath: string, envName: string, onLog: LogChannel): Promise<OperationResult> {
  return invoke("release", { repoPath, envName, onLog });
}

export function approvalApprove(
  repoPath: string,
  requestId: string,
  comment: string | undefined,
  onLog: LogChannel
): Promise<OperationResult> {
  return invoke("approval_approve", { repoPath, requestId, comment: comment ?? null, onLog });
}

export function approvalReject(
  repoPath: string,
  requestId: string,
  reason: string,
  onLog: LogChannel
): Promise<OperationResult> {
  return invoke("approval_reject", { repoPath, requestId, reason, onLog });
}

export function approvalCancel(repoPath: string, requestId: string, onLog: LogChannel): Promise<OperationResult> {
  return invoke("approval_cancel", { repoPath, requestId, onLog });
}

export function approvalRefresh(repoPath: string, requestId: string, onLog: LogChannel): Promise<OperationResult> {
  return invoke("approval_refresh", { repoPath, requestId, onLog });
}

export function approvalsList(repoPath: string): Promise<ApprovalsList> {
  return invoke("approvals_list", { repoPath });
}
