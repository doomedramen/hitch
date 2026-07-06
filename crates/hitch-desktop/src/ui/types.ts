export type RepoIdentity =
  | { kind: "origin"; origin_url_normalized: string }
  | { kind: "root"; root_commit_oid: string }
  | { kind: "unknown" };

export type RepoEntry = {
  id: string;
  path: string;
  display_name: string;
  expected_identity: RepoIdentity;
  added_at: string;
  last_opened_at?: string;
};

export type RepoProbeResult =
  | {
      ok: true;
      path: string;
      display_name: string;
      origin_url_normalized?: string;
      root_commit_oid?: string;
    }
  | { ok: false; path: string; error: string };

export type OutputLevel = "Info" | "Success" | "Warning" | "Error";

export type BufferedLine = {
  level: OutputLevel;
  message: string;
};

export type OperationResult = {
  ok: boolean;
  error?: string;
  lines: BufferedLine[];
};

export type TimelineKind = "GitCommit" | "HitchEvent";

export type TimelineItem = {
  when: string;
  kind: TimelineKind;
  summary: string;
  detail?: string | null;
};

export type BranchRow = {
  name: string;
  local: boolean;
  remote: boolean;
  is_environment: boolean;
  promoted_to: string[];
  base_for: string[];
};

export type EnvironmentMinModel = {
  name: string;
  base: string;
  promoted_count: number;
  locked: boolean;
  requires_approval: boolean;
  min_approvals: number;
  approvers_count: number;
};

export type WorkspaceIndexModel = {
  current_branch?: string | null;
  environments: EnvironmentMinModel[];
  promoted_branches: BranchRow[];
  branches: BranchRow[];
};

export type BranchDetailsModel = {
  branch: BranchRow;
  branch_sha: string;
  metadata_sha?: string | null;
  overview: string;
  timeline: TimelineItem[];
};

export type EnvironmentDetailsModel = {
  name: string;
  env_sha: string;
  metadata_sha?: string | null;
  overview: string;
  timeline: TimelineItem[];
};

export type ApprovalStatus = "Pending" | "Approved" | "Applied" | "Rejected" | "Cancelled";

export type ApprovalOperation = "Promote" | "Demote";

export type Approval = {
  approved_by: string;
  approved_at: string;
  comment?: string | null;
};

export type Rejection = {
  rejected_by: string;
  rejected_at: string;
  reason: string;
};

export type ApprovalSnapshot = {
  base_branch: string;
  base_sha: string;
  /** [branch name, commit SHA] pairs, sorted by branch name. */
  branch_shas: [string, string][];
  merge_conflicts: boolean;
};

export type ApprovalRequest = {
  id: string;
  environment: string;
  branch: string;
  operation: ApprovalOperation;
  requested_by: string;
  requested_at: string;
  status: ApprovalStatus;
  approvals: Approval[];
  rejection?: Rejection | null;
  snapshot: ApprovalSnapshot;
  approvers: string[];
  approval_count: number;
  min_approvals: number;
  drifted_branches: string[];
  viewer_is_requester: boolean;
  viewer_has_approved: boolean;
  viewer_can_approve: boolean;
  viewer_can_reject: boolean;
  viewer_can_cancel: boolean;
  viewer_can_refresh: boolean;
  viewer_can_execute: boolean;
};

export type ApprovalsList = {
  current_user?: string | null;
  requests: ApprovalRequest[];
};

