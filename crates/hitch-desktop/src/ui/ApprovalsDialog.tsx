import React, { useEffect, useMemo, useState } from "react";
import {
  ArrowDownRight,
  ArrowUpRight,
  Ban,
  CheckCircle2,
  CircleX,
  Clock,
  GitBranch,
  LoaderCircle,
  RefreshCw,
  Rocket,
  ShieldCheck,
  TriangleAlert
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Sticker } from "./Sticker";
import { cn } from "@/lib/utils";
import type { ApprovalRequest, ApprovalStatus, ApprovalsList } from "./types";

/** An action the user requested on a specific approval request. */
export type ApprovalAction =
  | { kind: "approve"; request: ApprovalRequest; comment?: string }
  | { kind: "reject"; request: ApprovalRequest; reason: string }
  | { kind: "cancel"; request: ApprovalRequest }
  | { kind: "refresh"; request: ApprovalRequest }
  | { kind: "execute"; request: ApprovalRequest };

type Filter = "Pending" | "Approved" | "All";
type FormMode = "approve" | "reject" | "cancel" | "refresh";

const MIN_REASON_LENGTH = 10;

function shortSha(sha: string): string {
  return sha.slice(0, 7);
}

function statusMeta(status: ApprovalStatus): {
  Icon: typeof Clock;
  className: string;
} {
  switch (status) {
    case "Pending":
      return { Icon: Clock, className: "bg-[var(--warn-soft)] text-warn" };
    case "Approved":
      return { Icon: CheckCircle2, className: "bg-[rgba(0,122,255,0.14)] text-primary" };
    case "Applied":
      return { Icon: Rocket, className: "bg-[var(--success-soft)] text-success" };
    case "Rejected":
      return { Icon: CircleX, className: "bg-destructive/15 text-destructive" };
    case "Cancelled":
      return { Icon: Ban, className: "bg-[var(--fill-soft)] text-label-secondary" };
  }
}

function ProgressBar({ count, total }: { count: number; total: number }) {
  const pct = total > 0 ? Math.min(100, Math.round((count / total) * 100)) : 100;
  return (
    <div className="flex items-center gap-2.5">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--fill-soft)]">
        <div className="h-full rounded-full bg-primary transition-all" style={{ width: `${pct}%` }} />
      </div>
      <div className="shrink-0 text-[11px] font-medium tabular-nums text-label-secondary">
        {count}/{total}
      </div>
    </div>
  );
}

function textareaClass(): string {
  return cn(
    "flex min-h-[64px] w-full rounded-[6px] border-[0.5px] border-separator-strong bg-[var(--control-bg)] px-2.5 py-2 text-[13px] text-label shadow-control",
    "placeholder:text-label-tertiary focus-visible:outline-none focus-visible:border-transparent focus-visible:ring-2 focus-visible:ring-ring/50"
  );
}

function RequestCard({
  request,
  dateFmt,
  active,
  comment,
  reason,
  onComment,
  onReason,
  onOpenForm,
  onCloseForm,
  onSubmit
}: {
  request: ApprovalRequest;
  dateFmt: Intl.DateTimeFormat;
  active: FormMode | null;
  comment: string;
  reason: string;
  onComment: (v: string) => void;
  onReason: (v: string) => void;
  onOpenForm: (mode: FormMode) => void;
  onCloseForm: () => void;
  onSubmit: (action: ApprovalAction) => void;
}) {
  const { Icon: StatusIcon, className: statusClass } = statusMeta(request.status);
  const drifted = request.drifted_branches.length > 0;
  const OpIcon = request.operation === "Promote" ? ArrowUpRight : ArrowDownRight;

  const approvedBy = useMemo(
    () => new Set(request.approvals.map((a) => a.approved_by)),
    [request.approvals]
  );

  const reasonValid = reason.trim().length >= MIN_REASON_LENGTH;

  return (
    <div className="overflow-hidden rounded-[10px] hairline-strong bg-card">
      {/* Header */}
      <div className="flex items-start justify-between gap-3 p-3 hairline-b">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[14px] font-semibold tracking-tight text-label">
            <OpIcon className="h-4 w-4 shrink-0 text-label-secondary" strokeWidth={2} aria-hidden="true" />
            <span className="truncate">{request.branch}</span>
            <span className="text-label-tertiary">→</span>
            <span className="truncate">{request.environment}</span>
          </div>
          <div className="mt-0.5 text-[12px] text-label-secondary">
            {request.operation} · by {request.requested_by} · {dateFmt.format(new Date(request.requested_at))}
          </div>
        </div>
        <Sticker className={statusClass}>
          <StatusIcon className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
          {request.status}
        </Sticker>
      </div>

      <div className="space-y-3 p-3">
        {/* Approval progress */}
        <div className="space-y-2">
          <ProgressBar count={request.approval_count} total={request.min_approvals} />
          {request.approvers.length > 0 ? (
            <ul className="flex flex-wrap gap-1.5">
              {request.approvers.map((a) => {
                const has = approvedBy.has(a);
                const isRequester = a === request.requested_by;
                return (
                  <li
                    key={a}
                    className={cn(
                      "inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px]",
                      has ? "bg-[var(--success-soft)] text-success" : "hairline text-label-secondary"
                    )}
                    title={isRequester ? `${a} (requester — cannot self-approve)` : a}
                  >
                    {has ? (
                      <CheckCircle2 className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                    ) : (
                      <Clock className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                    )}
                    <span className="max-w-[18ch] truncate">{a}</span>
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>

        {/* Approval comments */}
        {request.approvals.some((a) => a.comment && a.comment.trim().length > 0) ? (
          <ul className="space-y-1 border-l-2 border-separator pl-2.5">
            {request.approvals
              .filter((a) => a.comment && a.comment.trim().length > 0)
              .map((a, i) => (
                <li key={i} className="text-[12px] text-label">
                  <span className="text-label-secondary">{a.approved_by}: </span>
                  {a.comment}
                </li>
              ))}
          </ul>
        ) : null}

        {/* Rejection detail */}
        {request.rejection ? (
          <div className="rounded-[8px] hairline bg-destructive/10 p-2.5 text-[12px]">
            <div className="font-medium text-destructive">Rejected by {request.rejection.rejected_by}</div>
            <div className="mt-0.5 text-label">{request.rejection.reason}</div>
          </div>
        ) : null}

        {/* Drift warning */}
        {drifted ? (
          <div className="flex items-start gap-2 rounded-[8px] hairline bg-[var(--warn-soft)] p-2.5 text-[12px] text-label">
            <TriangleAlert className="mt-0.5 h-4 w-4 shrink-0 text-warn" strokeWidth={2} aria-hidden="true" />
            <div>
              <div className="font-medium text-warn">Snapshot drifted</div>
              <div className="font-mono text-label-secondary">{request.drifted_branches.join(", ")}</div>
              {request.viewer_can_refresh ? (
                <div className="mt-0.5 text-label-secondary">
                  Refresh the snapshot to let reviewers re-approve the new code.
                </div>
              ) : null}
            </div>
          </div>
        ) : null}

        {/* Snapshot summary */}
        <div className="flex flex-wrap items-center gap-2 text-[11px] text-label-tertiary">
          <span className="inline-flex items-center gap-1">
            <GitBranch className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
            {request.snapshot.base_branch}@{shortSha(request.snapshot.base_sha)}
          </span>
          {request.snapshot.merge_conflicts ? (
            <span className="inline-flex items-center gap-1 text-destructive">
              <TriangleAlert className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
              conflicts
            </span>
          ) : null}
        </div>

        {/* Action buttons */}
        {active === null ? (
          <div className="flex flex-wrap gap-2">
            {request.viewer_can_approve ? (
              <Button
                size="sm"
                variant="default"
                disabled={drifted}
                title={drifted ? "Refresh the drifted snapshot before approving" : undefined}
                onClick={() => onOpenForm("approve")}
              >
                <ShieldCheck className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Approve
              </Button>
            ) : null}
            {request.viewer_can_execute ? (
              <Button size="sm" variant="default" onClick={() => onSubmit({ kind: "execute", request })}>
                <Rocket className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Apply now
              </Button>
            ) : null}
            {request.viewer_can_reject ? (
              <Button size="sm" variant="destructive" onClick={() => onOpenForm("reject")}>
                <CircleX className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Reject
              </Button>
            ) : null}
            {request.viewer_can_refresh ? (
              <Button size="sm" variant="secondary" onClick={() => onOpenForm("refresh")}>
                <RefreshCw className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Refresh snapshot
              </Button>
            ) : null}
            {request.viewer_can_cancel ? (
              <Button size="sm" variant="ghost" onClick={() => onOpenForm("cancel")}>
                <Ban className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Cancel request
              </Button>
            ) : null}
          </div>
        ) : null}

        {/* Approve form */}
        {active === "approve" ? (
          <div className="space-y-2 rounded-[8px] hairline bg-[var(--fill-soft)] p-2.5">
            <div className="text-[11px] font-medium text-label-secondary">Approve — optional comment</div>
            <textarea
              className={textareaClass()}
              placeholder="Looks good to me…"
              value={comment}
              onChange={(e) => onComment(e.target.value)}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={onCloseForm}>
                Cancel
              </Button>
              <Button
                size="sm"
                variant="default"
                onClick={() =>
                  onSubmit({ kind: "approve", request, comment: comment.trim() || undefined })
                }
              >
                <ShieldCheck className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Confirm approval
              </Button>
            </div>
          </div>
        ) : null}

        {/* Reject form */}
        {active === "reject" ? (
          <div className="space-y-2 rounded-[8px] hairline bg-[var(--fill-soft)] p-2.5">
            <div className="text-[11px] font-medium text-label-secondary">Reject — reason required</div>
            <textarea
              className={textareaClass()}
              placeholder="Explain what needs to change (at least 10 characters)…"
              value={reason}
              onChange={(e) => onReason(e.target.value)}
              autoFocus
            />
            <div className="flex items-center justify-between gap-2">
              <div
                className={cn(
                  "text-[11px] tabular-nums",
                  reasonValid ? "text-label-tertiary" : "text-destructive"
                )}
              >
                {reason.trim().length}/{MIN_REASON_LENGTH} min
              </div>
              <div className="flex gap-2">
                <Button size="sm" variant="ghost" onClick={onCloseForm}>
                  Cancel
                </Button>
                <Button
                  size="sm"
                  variant="destructive"
                  disabled={!reasonValid}
                  onClick={() => onSubmit({ kind: "reject", request, reason: reason.trim() })}
                >
                  <CircleX className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                  Confirm rejection
                </Button>
              </div>
            </div>
          </div>
        ) : null}

        {/* Cancel confirm */}
        {active === "cancel" ? (
          <div className="space-y-2 rounded-[8px] hairline bg-[var(--fill-soft)] p-2.5">
            <div className="text-[13px] text-label">Cancel this request? This can’t be undone.</div>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={onCloseForm}>
                Keep it
              </Button>
              <Button size="sm" variant="destructive" onClick={() => onSubmit({ kind: "cancel", request })}>
                <Ban className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Confirm cancel
              </Button>
            </div>
          </div>
        ) : null}

        {/* Refresh confirm */}
        {active === "refresh" ? (
          <div className="space-y-2 rounded-[8px] hairline bg-[var(--fill-soft)] p-2.5">
            <div className="text-[13px] text-label">
              Re-snapshot with current branch SHAs? This clears all existing approvals — reviewers must
              re-approve.
            </div>
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={onCloseForm}>
                Cancel
              </Button>
              <Button size="sm" variant="secondary" onClick={() => onSubmit({ kind: "refresh", request })}>
                <RefreshCw className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                Confirm refresh
              </Button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function ApprovalsDialog({
  open,
  onOpenChange,
  approvals,
  loading,
  error,
  dateFmt,
  onAction,
  onRefresh
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  approvals: ApprovalsList | null;
  loading: boolean;
  error: string | null;
  dateFmt: Intl.DateTimeFormat;
  onAction: (action: ApprovalAction) => void;
  onRefresh: () => void;
}) {
  const [filter, setFilter] = useState<Filter>("Pending");
  const [activeId, setActiveId] = useState<string | null>(null);
  const [activeMode, setActiveMode] = useState<FormMode | null>(null);
  const [comment, setComment] = useState("");
  const [reason, setReason] = useState("");

  const closeForm = () => {
    setActiveId(null);
    setActiveMode(null);
    setComment("");
    setReason("");
  };

  // Collapse any open editor when the dialog closes or the filter changes.
  useEffect(() => {
    if (!open) closeForm();
  }, [open]);
  useEffect(() => {
    closeForm();
  }, [filter]);

  const requests = approvals?.requests ?? [];
  const pendingCount = useMemo(
    () => requests.filter((r) => r.status === "Pending").length,
    [requests]
  );
  const approvedCount = useMemo(
    () => requests.filter((r) => r.status === "Approved").length,
    [requests]
  );

  const visible = useMemo(() => {
    if (filter === "All") return requests;
    return requests.filter((r) => r.status === filter);
  }, [requests, filter]);

  const filters: { key: Filter; label: string; count?: number }[] = [
    { key: "Pending", label: "Pending", count: pendingCount },
    { key: "Approved", label: "Approved", count: approvedCount },
    { key: "All", label: "All", count: requests.length }
  ];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <div className="flex items-center justify-between gap-3">
            <DialogTitle className="flex items-center gap-2">
              <ShieldCheck className="h-[18px] w-[18px]" strokeWidth={2} aria-hidden="true" />
              Approvals
            </DialogTitle>
            <Button size="sm" variant="secondary" onClick={onRefresh} disabled={loading}>
              {loading ? (
                <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
              ) : (
                <RefreshCw className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
              )}
              Refresh
            </Button>
          </div>
        </DialogHeader>

        {/* Filter (segmented control) */}
        <div className="inline-flex w-fit items-center gap-0.5 rounded-[7px] bg-[var(--seg-bg)] p-0.5">
          {filters.map((f) => (
            <button
              key={f.key}
              type="button"
              onClick={() => setFilter(f.key)}
              className={cn(
                "inline-flex items-center gap-1.5 rounded-[5px] px-2.5 py-1 text-[12px] font-medium transition-colors",
                filter === f.key
                  ? "bg-[var(--control-bg)] text-label shadow-control"
                  : "text-label-secondary hover:text-label"
              )}
            >
              {f.label}
              {typeof f.count === "number" ? (
                <span className={cn(filter === f.key ? "text-label-tertiary" : "text-label-tertiary")}>
                  {f.count}
                </span>
              ) : null}
            </button>
          ))}
        </div>

        {error ? (
          <div className="rounded-[8px] bg-destructive/10 p-3 text-[13px] text-destructive">{error}</div>
        ) : null}

        <ScrollArea className="h-[55vh]">
          <div className="space-y-3 pr-3">
            {loading && requests.length === 0 ? (
              <div className="flex items-center gap-2 py-8 text-[13px] text-label-secondary">
                <LoaderCircle className="h-5 w-5 animate-spin" aria-hidden="true" />
                Loading approvals…
              </div>
            ) : visible.length === 0 ? (
              <div className="flex flex-col items-center justify-center gap-2 py-16">
                <ShieldCheck className="h-8 w-8 text-label-tertiary" strokeWidth={1.6} aria-hidden="true" />
                <div className="text-[13px] text-label-tertiary">
                  No {filter === "All" ? "" : filter.toLowerCase() + " "}requests
                </div>
              </div>
            ) : (
              visible.map((request) => (
                <RequestCard
                  key={request.id}
                  request={request}
                  dateFmt={dateFmt}
                  active={activeId === request.id ? activeMode : null}
                  comment={comment}
                  reason={reason}
                  onComment={setComment}
                  onReason={setReason}
                  onOpenForm={(mode) => {
                    setActiveId(request.id);
                    setActiveMode(mode);
                    setComment("");
                    setReason("");
                  }}
                  onCloseForm={closeForm}
                  onSubmit={(action) => {
                    closeForm();
                    onAction(action);
                  }}
                />
              ))
            )}
          </div>
        </ScrollArea>

        <div className="text-[11px] text-label-tertiary">
          {approvals?.current_user
            ? `Acting as ${approvals.current_user}`
            : "No git user email configured — actions are disabled"}
        </div>
      </DialogContent>
    </Dialog>
  );
}
