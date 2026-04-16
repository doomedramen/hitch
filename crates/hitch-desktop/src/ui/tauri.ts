import { invoke } from "@tauri-apps/api/core";
import type {
  BranchDetailsModel,
  EnvironmentDetailsModel,
  OperationResult,
  RepoProbeResult,
  WorkspaceIndexModel
} from "./types";

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

export function promote(repoPath: string, branchName: string, envName: string): Promise<OperationResult> {
  return invoke("promote", { repoPath, branchName, envName });
}

export function rebuild(repoPath: string, envName: string, force: boolean): Promise<OperationResult> {
  return invoke("rebuild", { repoPath, envName, force });
}

export function release(repoPath: string, envName: string): Promise<OperationResult> {
  return invoke("release", { repoPath, envName });
}
