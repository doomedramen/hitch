import { getCurrentWindow } from "@tauri-apps/api/window";
import { Channel } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import {
  ArrowUpRight,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Cloud,
  FileText,
  FolderSearch,
  GitBranch,
  History,
  HardDrive,
  Info,
  Layers,
  LoaderCircle,
  LockKeyhole,
  Maximize2,
  Minus as MinusIcon,
  Monitor,
  RefreshCw,
  Rocket,
  Search,
  ShieldCheck,
  Trash2,
  TriangleAlert,
  X
} from "lucide-react";
import { loadRepos, saveRepos } from "./storage";
import type {
  ApprovalsList,
  BranchDetailsModel,
  BufferedLine,
  EnvironmentDetailsModel,
  OperationResult,
  RepoEntry,
  RepoIdentity,
  WorkspaceIndexModel
} from "./types";
import {
  approvalApprove,
  approvalCancel,
  approvalRefresh,
  approvalReject,
  approvalsList,
  branchDetails,
  envDetails,
  promote,
  rebuild,
  release,
  repoProbe,
  workspaceIndex
} from "./tauri";
import { OutputLevelIcon, RepoIdentityIcon, TimelineKindIcon } from "./icons";
import { HitchIcon, SidebarRowButton, Sticker, TitleBar, AboutDialog, ApprovalsDialog } from "./index";
import type { ApprovalAction } from "./ApprovalsDialog";

const appWindow = getCurrentWindow();

type Selection =
  | { kind: "none" }
  | { kind: "env"; name: string }
  | { kind: "branch"; name: string; isEnvironment: boolean };

type ConfirmState = null | {
  title: string;
  description: string;
  actionLabel: string;
  actionVariant?: "default" | "outline" | "secondary" | "ghost" | "destructive";
  onAction: () => void;
};

function nowIso(): string {
  return new Date().toISOString();
}

function uuid(): string {
  // Good-enough local ID (desktop-only, no security needs).
  return crypto.randomUUID();
}

function identityFromProbe(probe: { origin_url_normalized?: string; root_commit_oid?: string }): RepoIdentity {
  if (probe.origin_url_normalized) return { kind: "origin", origin_url_normalized: probe.origin_url_normalized };
  if (probe.root_commit_oid) return { kind: "root", root_commit_oid: probe.root_commit_oid };
  return { kind: "unknown" };
}

function expectedIdentityLabel(id: RepoIdentity): string {
  if (id.kind === "origin") return id.origin_url_normalized;
  if (id.kind === "root") return id.root_commit_oid.slice(0, 10);
  return "unknown";
}

function identityMatches(expected: RepoIdentity, found: RepoIdentity): boolean {
  if (expected.kind === "origin" && found.kind === "origin") {
    return expected.origin_url_normalized === found.origin_url_normalized;
  }
  if (expected.kind === "root" && found.kind === "root") {
    return expected.root_commit_oid === found.root_commit_oid;
  }
  // If we don't have a strong expectation, don't block.
  if (expected.kind === "unknown") return true;
  return false;
}

function selectionEquals(a: Selection, b: Selection): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "none") return true;
  if (a.kind === "env" && b.kind === "env") return a.name === b.name;
  if (a.kind === "branch" && b.kind === "branch") return a.name === b.name && a.isEnvironment === b.isEnvironment;
  return false;
}

type OverviewRow =
  | { kind: "kv"; key: string; value: string }
  | { kind: "list"; key: string; items: string[] }
  | { kind: "text"; value: string };

function SidebarSectionHeading({
  className,
  ...props
}: React.HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn(
        "px-2 pt-3 pb-1 text-[11px] font-semibold text-label-secondary",
        className
      )}
      {...props}
    />
  );
}

function parseOverview(text: string): OverviewRow[] {
  const lines = text
    .split("\n")
    .map((l) => l.replace(/\r$/, ""))
    .filter((l) => l.trim().length > 0);

  const out: OverviewRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];

    // Section header like "branches:" followed by indented "- ..." lines.
    if (line.endsWith(":") && !line.includes(": ")) {
      const key = line.slice(0, -1).trim();
      const items: string[] = [];
      let j = i + 1;
      while (j < lines.length) {
        const next = lines[j];
        const trimmed = next.trimStart();
        const isIndented = next.length !== trimmed.length;
        if (!isIndented) break;
        const item = trimmed.replace(/^- /, "").trim();
        if (item.length > 0) items.push(item);
        j += 1;
      }
      out.push({ kind: "list", key, items });
      i = j;
      continue;
    }

    const colon = line.indexOf(":");
    if (colon > 0) {
      const key = line.slice(0, colon).trim();
      const value = line.slice(colon + 1).trim();
      out.push({ kind: "kv", key, value });
      i += 1;
      continue;
    }

    out.push({ kind: "text", value: line.trim() });
    i += 1;
  }

  return out;
}

function looksCodey(value: string): boolean {
  const v = value.trim();
  if (v.length >= 7 && /^[0-9a-f]{7,40}$/i.test(v)) return true;
  if (v.includes("/") || v.includes("..") || v.includes("→")) return true;
  if (v.startsWith("refs/")) return true;
  return false;
}

// Overview keys whose value is an ISO timestamp we localize + humanize.
const TIME_KEYS = new Set(["rebuilt_at", "released_at", "locked_at"]);

/** Local, human time: "just now" / "2 minutes ago" for recent, else local date/time. */
function formatWhen(iso: string, dateFmt: Intl.DateTimeFormat): string {
  const d = new Date(iso);
  const ms = d.getTime();
  if (!Number.isFinite(ms)) return iso;
  const diff = Date.now() - ms; // >0 = past
  const abs = Math.abs(diff);
  const MIN = 60_000;
  const HOUR = 3_600_000;
  const DAY = 86_400_000;
  if (abs < 45_000) return "just now";
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  if (abs < HOUR) return rtf.format(-Math.round(diff / MIN), "minute");
  if (abs < DAY) return rtf.format(-Math.round(diff / HOUR), "hour");
  if (abs < 7 * DAY) return rtf.format(-Math.round(diff / DAY), "day");
  return dateFmt.format(d);
}

function overviewKeyLabel(key: string): string {
  switch (key.trim().toLowerCase()) {
    case "diff_stat":
      return "Diff";
    case "rebuilt_at":
      return "Rebuilt";
    case "released_at":
      return "Released";
    case "locked_at":
      return "Locked";
    case "locked_by":
      return "Locked by";
    default:
      return key;
  }
}

function DiffStatList({ items }: { items: string[] }) {
  const summary = items.find((l) => l.includes("files changed"));
  const rows = items.filter((l) => l !== summary);

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-x-4 gap-y-1">
        {rows.map((line, j) => {
          const parts = line.split("|");
          const file = (parts[0] ?? "").trim();
          const stat = (parts[1] ?? "").trim();
          return (
            <React.Fragment key={j}>
              <div className="min-w-0 truncate text-[13px] text-label">{file.length > 0 ? file : line}</div>
              <div className="text-[11px] font-mono text-label-secondary bg-[var(--fill-soft)] px-1.5 py-0.5 rounded-[4px]">{stat.length > 0 ? stat : ""}</div>
            </React.Fragment>
          );
        })}
      </div>
      {summary ? <div className="text-[11px] font-medium text-label-secondary inline-block">{summary.trim()}</div> : null}
    </div>
  );
}

function OverviewPanel({ text, dateFmt }: { text: string; dateFmt: Intl.DateTimeFormat }) {
  const rows = useMemo(() => parseOverview(text), [text]);
  if (rows.length === 0) return <div className="py-12 text-[13px] text-label-tertiary text-center">No overview</div>;

  return (
    <div className="select-text">
      {rows.map((row, idx) => {
        const showDivider = idx !== rows.length - 1;
        if (row.kind === "text") {
          return (
            <div key={idx} className={cn("py-2.5 text-[13px] text-label", showDivider ? "hairline-b" : "")}>
              {row.value}
            </div>
          );
        }

        if (row.kind === "list") {
          const isDiffStat = row.key.trim().toLowerCase() === "diff_stat";
          return (
            <div key={idx} className={cn("py-2.5", showDivider ? "hairline-b" : "")}>
              <div className="grid grid-cols-[140px_1fr] gap-3">
                <div className="pt-0.5 text-[12px] text-label-secondary">
                  {overviewKeyLabel(row.key)}
                </div>
                <div className="min-w-0">
                  {row.items.length === 0 ? (
                    <div className="text-[13px] text-label-tertiary">None</div>
                  ) : isDiffStat ? (
                    <DiffStatList items={row.items} />
                  ) : (
                    <ul className="space-y-1">
                      {row.items.map((it, j) => (
                        <li key={j} className="flex gap-2 text-[13px] text-label">
                          <span className="mt-2 h-1 w-1 shrink-0 rounded-full bg-label-tertiary" />
                          <span className={cn("min-w-0 break-words", looksCodey(it) ? "font-mono text-[12px] text-label-secondary" : "")}>{it}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </div>
            </div>
          );
        }

        const isYes = row.value.toLowerCase() === "yes";
        const isNo = row.value.toLowerCase() === "no";
        const badge =
          row.key === "locked" && (isYes || isNo) ? (
            <span
              className={cn(
                "inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium",
                isYes ? "bg-[var(--warn-soft)] text-warn" : "bg-[var(--fill-soft)] text-label-secondary"
              )}
            >
              {row.value}
            </span>
          ) : null;

        const isTime = TIME_KEYS.has(row.key.trim().toLowerCase());
        const shownValue = isTime ? formatWhen(row.value, dateFmt) : row.value;

        return (
          <div key={idx} className={cn("py-2.5", showDivider ? "hairline-b" : "")}>
            <div className="grid grid-cols-[140px_1fr] gap-3">
              <div className="pt-0.5 text-[12px] text-label-secondary">
                {overviewKeyLabel(row.key)}
              </div>
              <div className="min-w-0 text-[13px] text-label">
                {badge ?? (
                  <span className={cn("break-words", !isTime && looksCodey(shownValue) ? "font-mono text-[12px] text-label-secondary" : "")}>
                    {shownValue.length === 0 ? <span className="text-label-tertiary">—</span> : shownValue}
                  </span>
                )}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}

function repoOwnerGroup(repo: RepoEntry): string {
  if (repo.expected_identity.kind !== "origin") return "Local";
  const parts = repo.expected_identity.origin_url_normalized.split("/");
  if (parts.length >= 3 && parts[1]?.trim().length > 0) return parts[1];
  return "Local";
}

function repoSortStamp(repo: RepoEntry): number {
 const ts = repo.last_opened_at ?? repo.added_at;
 const ms = Date.parse(ts);
 return Number.isFinite(ms) ? ms : 0;
}

export function App() {  const [repos, setRepos] = useState<RepoEntry[]>([]);
  const [selectedRepoId, setSelectedRepoId] = useState<string | null>(null);
  const [repoListOpen, setRepoListOpen] = useState<boolean>(false);
  const [repoFilter, setRepoFilter] = useState<string>("");
  const [aboutOpen, setAboutOpen] = useState<boolean>(false);

  const selectedRepo = useMemo(() => repos.find((r) => r.id === selectedRepoId) ?? null, [repos, selectedRepoId]);
  const selectedRepoPath = selectedRepo?.path ?? null;

  const [repoMissing, setRepoMissing] = useState<null | { repo: RepoEntry; error: string }>(null);
  const [repoLocateMismatch, setRepoLocateMismatch] = useState<
    null | { repo: RepoEntry; foundPath: string; expected: RepoIdentity; found: RepoIdentity }
  >(null);

  const [status, setStatus] = useState<string>("Ready");

  const [index, setIndex] = useState<WorkspaceIndexModel | null>(null);
  const [indexLoading, setIndexLoading] = useState<boolean>(false);
  const [indexError, setIndexError] = useState<string | null>(null);

  const [filter, setFilter] = useState<string>("");

  const [selection, setSelection] = useState<Selection>({ kind: "none" });
  const [desiredSelection, setDesiredSelection] = useState<Selection>({ kind: "none" });
  const [renderedSelection, setRenderedSelection] = useState<Selection>({ kind: "none" });
  const [tab, setTab] = useState<"overview" | "timeline">("overview");

  const [branchModel, setBranchModel] = useState<BranchDetailsModel | null>(null);
  const [envModel, setEnvModel] = useState<EnvironmentDetailsModel | null>(null);
  const [detailsLoading, setDetailsLoading] = useState<boolean>(false);
  const [detailsError, setDetailsError] = useState<string | null>(null);

  const [op, setOp] = useState<null | { title: string; lines?: BufferedLine[]; result?: OperationResult }>(null);
  const [promotePicker, setPromotePicker] = useState<null | { branch: string }>(null);
  const [confirm, setConfirm] = useState<ConfirmState>(null);

  const [approvals, setApprovals] = useState<ApprovalsList | null>(null);
  const [approvalsLoading, setApprovalsLoading] = useState<boolean>(false);
  const [approvalsError, setApprovalsError] = useState<string | null>(null);
  const [approvalsOpen, setApprovalsOpen] = useState<boolean>(false);

  const pendingApprovalCount = useMemo(
    () => (approvals?.requests ?? []).filter((r) => r.status === "Pending").length,
    [approvals]
  );

  useEffect(() => {
    void (async () => {
      const r = await loadRepos();
      setRepos(r);
      if (r[0]) {
        void selectRepo(r[0]);
      }
    })();
  }, []);

  const dateFmt = useMemo(
    () => new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }),
    []
  );

  const repoListModel = useMemo(() => {
    const needle = repoFilter.trim().toLowerCase();
    const all = needle.length === 0
      ? repos
      : repos.filter((r) => {
          const owner = repoOwnerGroup(r).toLowerCase();
          return (
            r.display_name.toLowerCase().includes(needle) ||
            owner.includes(needle)
          );
        });

    const sortedByRecent = [...all].sort((a, b) => repoSortStamp(b) - repoSortStamp(a));

    if (needle.length > 0) {
      return { mode: "filtered" as const, results: sortedByRecent };
    }

    const recent = sortedByRecent.slice(0, 6);
    const recentIds = new Set(recent.map((r) => r.id));

    const groups = new Map<string, RepoEntry[]>();
    for (const r of repos) {
      if (recentIds.has(r.id)) continue;
      const g = repoOwnerGroup(r);
      const list = groups.get(g) ?? [];
      list.push(r);
      groups.set(g, list);
    }
    for (const [, list] of groups) {
      list.sort((a, b) => a.display_name.localeCompare(b.display_name));
    }
    const groupNames = [...groups.keys()].sort((a, b) => a.localeCompare(b));

    return { mode: "default" as const, recent, groups, groupNames };
  }, [repos, repoFilter]);

  function persist(next: RepoEntry[]) {
    setRepos(next);
    void saveRepos(next);
  }

  function persistWith(update: (prev: RepoEntry[]) => RepoEntry[]) {
    setRepos((prev) => {
      const next = update(prev);
      void saveRepos(next);
      return next;
    });
  }

  useEffect(() => {
    if (!repoListOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setRepoListOpen(false);
        setRepoFilter("");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [repoListOpen]);

  function toggleRepoList() {
    setRepoListOpen((v) => {
      const next = !v;
      if (!next) setRepoFilter("");
      return next;
    });
  }

  function closeRepoList() {
    setRepoListOpen(false);
    setRepoFilter("");
  }

  async function addRepo() {
    const selected = await open({ directory: true, multiple: false, title: "Add repository…" });
    if (!selected || Array.isArray(selected)) return;
    setStatus("Checking repository…");
    const probe = await repoProbe(selected);
    if (!probe.ok) {
      setStatus("Add failed");
      setIndexError(probe.error);
      return;
    }

    const entry: RepoEntry = {
      id: uuid(),
      path: probe.path,
      display_name: probe.display_name,
      expected_identity: identityFromProbe(probe),
      added_at: nowIso(),
      last_opened_at: nowIso()
    };

    const next = [entry, ...repos.filter((r) => r.path !== entry.path)];
    persist(next);
    setSelectedRepoId(entry.id);
    setStatus("Repository added");
  }

  function removeRepo(id: string) {
    const next = repos.filter((r) => r.id !== id);
    persist(next);
    if (selectedRepoId === id) {
      setSelectedRepoId(next[0]?.id ?? null);
      setIndex(null);
      setSelection({ kind: "none" });
    }
  }

  async function locateRepo(repo: RepoEntry) {
    const selected = await open({ directory: true, multiple: false, title: "Locate repository…" });
    if (!selected || Array.isArray(selected)) return;
    setStatus("Checking selected folder…");
    const probe = await repoProbe(selected);
    if (!probe.ok) {
      setStatus("Locate failed");
      setIndexError(probe.error);
      return;
    }

    const foundIdentity = identityFromProbe(probe);
    const expected = repo.expected_identity;

    if (!identityMatches(expected, foundIdentity)) {
      setRepoLocateMismatch({ repo, foundPath: probe.path, expected, found: foundIdentity });
      return;
    }

    const next = repos.map((r) =>
      r.id === repo.id
        ? { ...r, path: probe.path, display_name: probe.display_name, expected_identity: foundIdentity, last_opened_at: nowIso() }
        : r
    );
    persist(next);
    setRepoMissing(null);
    setRepoLocateMismatch(null);
    setStatus("Repository updated");
  }

  async function selectRepo(repo: RepoEntry) {
    setSelectedRepoId(repo.id);
    closeRepoList();
    setIndex(null);
    setIndexError(null);
    setApprovals(null);
    setApprovalsError(null);
    setSelection({ kind: "none" });
    setDesiredSelection({ kind: "none" });
    setRenderedSelection({ kind: "none" });
    setBranchModel(null);
    setEnvModel(null);
    setDetailsLoading(false);
    setDetailsError(null);
    // Cancel/ignore any in-flight details loads.
    detailsReqIdRef.current += 1;
    detailsLoadingRef.current = false;
    queuedSelectionRef.current = null;

    setStatus("Opening repository…");
    const probe = await repoProbe(repo.path);
    if (!probe.ok) {
      setRepoMissing({ repo, error: probe.error });
      setStatus("Repository missing");
      return;
    }
    setRepoMissing(null);

    setIndexLoading(true);
    setIndexError(null);
    try {
      const idx = await workspaceIndex(repo.path);
      setIndex(idx);
      setStatus("Ready");
      void refreshApprovals(repo.path);
      persistWith((prev) =>
        prev.map((r) => (r.id === repo.id ? { ...r, last_opened_at: nowIso() } : r))
      );
    } catch (e: any) {
      setIndexError(e?.toString?.() ?? "Failed to load workspace");
      setStatus("Workspace load failed");
    } finally {
      setIndexLoading(false);
    }
  }

  const detailsLoadingRef = useRef<boolean>(false);
  const queuedSelectionRef = useRef<Selection | null>(null);
  const detailsReqIdRef = useRef<number>(0);

  const startDetailsLoad = useCallback(
    async (next: Selection, repoPath: string) => {
      if (next.kind === "none") return;
      detailsLoadingRef.current = true;
      setDetailsLoading(true);
      setDetailsError(null);

      const reqId = (detailsReqIdRef.current += 1);
      try {
        if (next.kind === "env") {
          const model = await envDetails(repoPath, next.name);
          if (reqId !== detailsReqIdRef.current) return;
          setEnvModel(model);
          setBranchModel(null);
          setRenderedSelection(next);
        } else if (next.kind === "branch") {
          const model = await branchDetails(repoPath, next.name);
          if (reqId !== detailsReqIdRef.current) return;
          setBranchModel(model);
          setEnvModel(null);
          setRenderedSelection(next);
        }
      } catch (e: any) {
        if (reqId !== detailsReqIdRef.current) return;
        setDetailsError(e?.toString?.() ?? "Failed to load details");
      } finally {
        if (reqId === detailsReqIdRef.current) {
          setDetailsLoading(false);
          detailsLoadingRef.current = false;
        }

        const queued = queuedSelectionRef.current;
        // Only start another load if we queued something different while loading.
        if (queued && !selectionEquals(queued, next)) {
          queuedSelectionRef.current = null;
          void startDetailsLoad(queued, repoPath);
        } else {
          queuedSelectionRef.current = null;
        }
      }
    },
    [setBranchModel, setEnvModel]
  );

  useEffect(() => {
    if (!selectedRepoPath) return;
    if (desiredSelection.kind === "none") return;

    const repoPath = selectedRepoPath;
    if (detailsLoadingRef.current) {
      queuedSelectionRef.current = desiredSelection;
      return;
    }
    void startDetailsLoad(desiredSelection, repoPath);
  }, [desiredSelection, selectedRepoPath, startDetailsLoad]);

  function requestDetails(next: Selection) {
    setSelection(next); // instant list highlight
    if (!selectionEquals(next, desiredSelection)) setDesiredSelection(next); // drives the loader
    // Keep old content visible; the loader will update renderedSelection when done.
  }

  const filteredIndex = useMemo(() => {
    if (!index) return null;
    const needle = filter.trim().toLowerCase();
    const keep = (name: string) => (needle.length === 0 ? true : name.toLowerCase().includes(needle));

    // Always hide internal hitch branches for now to keep the list focused.
    const hide = (name: string) => name === "hitch-metadata" || name.startsWith("hitch/");

    return {
      ...index,
      environments: index.environments.filter((e) => keep(e.name)),
      promoted_branches: index.promoted_branches.filter((b) => keep(b.name) && !hide(b.name)),
      branches: index.branches.filter((b) => keep(b.name) && !hide(b.name))
    } satisfies WorkspaceIndexModel;
  }, [index, filter]);

  const detailsKey = useMemo(() => {
    if (renderedSelection.kind === "env") return `env:${renderedSelection.name}:${tab}`;
    if (renderedSelection.kind === "branch") return `branch:${renderedSelection.name}:${tab}`;
    return `none:${tab}`;
  }, [renderedSelection, tab]);

  async function refreshApprovals(repoPath: string) {
    setApprovalsLoading(true);
    setApprovalsError(null);
    try {
      const list = await approvalsList(repoPath);
      setApprovals(list);
    } catch (e: any) {
      setApprovalsError(e?.toString?.() ?? "Failed to load approvals");
    } finally {
      setApprovalsLoading(false);
    }
  }

  async function runOperation(
    title: string,
    opFn: (onLog: Channel<BufferedLine>) => Promise<OperationResult>
  ) {
    const channel = new Channel<BufferedLine>();
    // Append streamed lines live, but stop once the final result has arrived.
    channel.onmessage = (line) => {
      setOp((prev) => (prev && !prev.result ? { ...prev, lines: [...(prev.lines ?? []), line] } : prev));
    };
    setOp({ title, lines: [] });
    setStatus(title);
    try {
      const result = await opFn(channel);
      setOp({ title, result });
      setStatus(result.ok ? "Done" : "Failed");
      // refresh index + approvals (promote can create a request; approvals ops
      // mutate the request list and, when applied, the environment itself)
      if (selectedRepo) {
        try {
          const idx = await workspaceIndex(selectedRepo.path);
          setIndex(idx);
          if (result.ok) setStatus("Ready");
        } catch {
          // ignore
        }
        void refreshApprovals(selectedRepo.path);
      }
    } catch (e: any) {
      setOp({ title, result: { ok: false, error: e?.toString?.() ?? "Operation failed", lines: [] } });
      setStatus("Failed");
    }
  }

  function handleApprovalAction(action: ApprovalAction) {
    if (!selectedRepo) return;
    const path = selectedRepo.path;
    const r = action.request;
    const label = `${r.branch} → ${r.environment}`;
    switch (action.kind) {
      case "approve":
        void runOperation(`Approve ${label}`, (onLog) => approvalApprove(path, r.id, action.comment, onLog));
        break;
      case "execute":
        void runOperation(`Apply ${label}`, (onLog) => approvalApprove(path, r.id, undefined, onLog));
        break;
      case "reject":
        void runOperation(`Reject ${label}`, (onLog) => approvalReject(path, r.id, action.reason, onLog));
        break;
      case "cancel":
        void runOperation(`Cancel ${label}`, (onLog) => approvalCancel(path, r.id, onLog));
        break;
      case "refresh":
        void runOperation(`Refresh ${label}`, (onLog) => approvalRefresh(path, r.id, onLog));
        break;
    }
  }

  const detailsOverview =
    renderedSelection.kind === "env"
      ? envModel?.overview ?? ""
      : renderedSelection.kind === "branch"
        ? branchModel?.overview ?? ""
        : "";
  const detailsTimeline =
    renderedSelection.kind === "env"
      ? envModel?.timeline ?? []
      : renderedSelection.kind === "branch"
        ? branchModel?.timeline ?? []
        : [];

  return (
    <div className="flex flex-col h-full w-full overflow-hidden text-label">
      <AboutDialog open={aboutOpen} onOpenChange={setAboutOpen} />
      <ApprovalsDialog
        open={approvalsOpen}
        onOpenChange={setApprovalsOpen}
        approvals={approvals}
        loading={approvalsLoading}
        error={approvalsError}
        dateFmt={dateFmt}
        onAction={handleApprovalAction}
        onRefresh={() => {
          if (selectedRepo) void refreshApprovals(selectedRepo.path);
        }}
      />

      <Tabs
        value={tab}
        onValueChange={(v) => setTab(v === "timeline" ? "timeline" : "overview")}
        className="flex min-h-0 flex-1 flex-col"
      >
        {/* Unified top toolbar — native macOS traffic lights sit in the left inset. */}
        <header
          data-tauri-drag-region
          className="flex h-12 shrink-0 items-center gap-2.5 material-toolbar hairline-b pl-[82px] pr-2"
        >
          <button
            type="button"
            onClick={toggleRepoList}
            className={cn(
              "flex h-7 min-w-0 max-w-[240px] items-center gap-2 rounded-[6px] bg-[var(--control-bg)] px-2 text-left shadow-control transition-colors",
              "hover:bg-[var(--control-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
            )}
          >
            <Monitor className="h-4 w-4 shrink-0 text-label-secondary" strokeWidth={1.8} aria-hidden="true" />
            <span className="truncate text-[13px] font-medium tracking-tight text-label">
              {selectedRepo ? selectedRepo.display_name : "Select a repository"}
            </span>
            {repoListOpen ? (
              <ChevronUp className="h-4 w-4 shrink-0 text-label-secondary" aria-hidden="true" />
            ) : (
              <ChevronDown className="h-4 w-4 shrink-0 text-label-secondary" aria-hidden="true" />
            )}
          </button>

          <TabsList>
            <TabsTrigger value="overview">
              <FileText className="h-3.5 w-3.5" strokeWidth={2} aria-hidden="true" />
              Overview
            </TabsTrigger>
            <TabsTrigger value="timeline">
              <History className="h-3.5 w-3.5" strokeWidth={2} aria-hidden="true" />
              Timeline
            </TabsTrigger>
          </TabsList>

          <div data-tauri-drag-region className="flex-1" />

          {status !== "Ready" ? (
            <div className="hidden text-[11px] text-label-secondary sm:block">{status}</div>
          ) : null}
          {selectedRepo ? (
            <Button
              variant="secondary"
              size="sm"
              className="h-7 justify-center"
              onClick={() => {
                void refreshApprovals(selectedRepo.path);
                setApprovalsOpen(true);
              }}
            >
              <ShieldCheck className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
              Approvals
              {pendingApprovalCount > 0 ? (
                <span className="ml-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold text-destructive-foreground">
                  {pendingApprovalCount}
                </span>
              ) : null}
            </Button>
          ) : null}
          {selectedRepo && selection.kind === "branch" && !selection.isEnvironment ? (
            <Button
              variant="default"
              size="sm"
              className="h-7 justify-center"
              aria-label="Promote"
              title="Promote…"
              onClick={() => setPromotePicker({ branch: selection.name })}
            >
              <ArrowUpRight className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
              <span className="hidden md:inline">Promote…</span>
            </Button>
          ) : null}
          {selectedRepo && selection.kind === "env" ? (
            <>
              <Button
                variant="secondary"
                size="sm"
                className="h-7 justify-center"
                aria-label="Rebuild"
                title="Rebuild"
                onClick={() =>
                  setConfirm({
                    title: `Rebuild ${selection.name}`,
                    description: `Rebuild environment '${selection.name}'?`,
                    actionLabel: "Rebuild",
                    actionVariant: "default",
                    onAction: () =>
                      void runOperation(`Rebuild ${selection.name}`, (onLog) =>
                        rebuild(selectedRepo.path, selection.name, false, onLog)
                      )
                  })
                }
              >
                <RefreshCw className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                <span className="hidden md:inline">Rebuild</span>
              </Button>
              <Button
                variant="destructive"
                size="sm"
                className="h-7 justify-center"
                aria-label="Release"
                title="Release"
                onClick={() =>
                  setConfirm({
                    title: `Release ${selection.name}`,
                    description: `Release environment '${selection.name}'? This can rewrite history of the target branch.`,
                    actionLabel: "Release",
                    actionVariant: "destructive",
                    onAction: () =>
                      void runOperation(`Release ${selection.name}`, (onLog) => release(selectedRepo.path, selection.name, onLog))
                  })
                }
              >
                <Rocket className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
                <span className="hidden md:inline">Release</span>
              </Button>
            </>
          ) : null}

          <button
            type="button"
            onClick={() => setAboutOpen(true)}
            title="About Hitch Desktop"
            aria-label="About Hitch Desktop"
            className="ml-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-[6px] text-label-secondary transition-colors hover:bg-[var(--fill-soft)] hover:text-label"
          >
            <Info className="h-[18px] w-[18px]" strokeWidth={1.8} aria-hidden="true" />
          </button>
        </header>

        <div className="flex min-h-0 flex-1 w-full overflow-hidden">
        <aside className="flex w-[236px] shrink-0 flex-col hairline-r material-sidebar">

        {repoListOpen ? (
          <>
            <div className="flex items-center gap-2 px-3 py-3">
              <div className="relative flex-1">
                <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-label-tertiary z-10" strokeWidth={2} aria-hidden="true" />
                <Input
                  placeholder="Filter"
                  className="pl-9"
                  value={repoFilter}
                  onChange={(e) => setRepoFilter(e.target.value)}
                />
              </div>
              <Button
                variant="outline"
                size="default"
                className="shrink-0 justify-center gap-1.5"
                onClick={() => {
                  closeRepoList();
                  void addRepo();
                }}
              >
                Add
                <ChevronDown className="h-4 w-4 text-label-secondary" aria-hidden="true" />
              </Button>
            </div>
            <Separator />
            <ScrollArea className="flex-1" scrollbarClassName="bg-white">
              <div className="space-y-1 p-2">
                {repos.length === 0 ? (
                  <div className="px-2 py-4 text-[12px] text-label-tertiary">No repositories yet</div>
                ) : repoListModel.mode === "filtered" ? (
                  <div className="space-y-1">
                    <SidebarSectionHeading>Results</SidebarSectionHeading>
                    {repoListModel.results.length === 0 ? (
                      <div className="px-2 py-4 text-[12px] text-label-tertiary">No matches</div>
                    ) : (
                      <div className="space-y-1">
                        {repoListModel.results.map((r) => (
                          <SidebarRowButton
                            key={r.id}
                            icon={<RepoIdentityIcon identity={r.expected_identity} className="h-4 w-4" />}
                            label={r.display_name}
                            selected={selectedRepoId === r.id}
                            trailing={selectedRepoId === r.id ? <span className="h-2.5 w-2.5 rounded-full bg-ring" /> : null}
                            onClick={() => void selectRepo(r)}
                            title={`${r.display_name}\n${r.path}\n${expectedIdentityLabel(r.expected_identity)}`}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                ) : (
                  <>
                    <div className="space-y-1">
                      <SidebarSectionHeading>Recent</SidebarSectionHeading>
                      {repoListModel.recent.length === 0 ? (
                        <div className="px-2 py-4 text-[12px] text-label-tertiary">No recent repositories</div>
                      ) : (
                        <div className="space-y-1">
                          {repoListModel.recent.map((r) => (
                            <SidebarRowButton
                              key={r.id}
                              icon={<RepoIdentityIcon identity={r.expected_identity} className="h-4 w-4" />}
                              label={r.display_name}
                              selected={selectedRepoId === r.id}
                              trailing={selectedRepoId === r.id ? <span className="h-2.5 w-2.5 rounded-full bg-ring" /> : null}
                              onClick={() => void selectRepo(r)}
                              title={`${r.display_name}\n${r.path}\n${expectedIdentityLabel(r.expected_identity)}`}
                            />
                          ))}
                        </div>
                      )}
                    </div>

                    {repoListModel.groupNames.map((group) => {
                      const list = repoListModel.groups.get(group) ?? [];
                      if (list.length === 0) return null;
                      return (
                        <div key={group} className="space-y-1">
                          <SidebarSectionHeading>{group}</SidebarSectionHeading>
                          <div className="space-y-1">
                            {list.map((r) => (
                              <SidebarRowButton
                                key={r.id}
                                icon={<RepoIdentityIcon identity={r.expected_identity} className="h-4 w-4" />}
                                label={r.display_name}
                                selected={selectedRepoId === r.id}
                                trailing={selectedRepoId === r.id ? <span className="h-2.5 w-2.5 rounded-full bg-ring" /> : null}
                                onClick={() => void selectRepo(r)}
                                title={`${r.display_name}\n${r.path}\n${expectedIdentityLabel(r.expected_identity)}`}
                              />
                            ))}
                          </div>
                        </div>
                      );
                    })}
                  </>
                )}
              </div>
            </ScrollArea>
          </>
        ) : (
          <>
            <div className="space-y-2 px-3 py-3">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-label-tertiary z-10" strokeWidth={2} aria-hidden="true" />
                <Input
                  placeholder="Filter"
                  className="pl-9"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              </div>

              {!filteredIndex && !indexLoading && !indexError && !selectedRepo ? (
                <div className="text-[12px] text-label-tertiary px-3">No workspace loaded.</div>
              ) : null}
            </div>
            <Separator />

            <ScrollArea className="flex-1" scrollbarClassName="bg-white">
              <div className="space-y-1 p-2">
                {indexLoading ? (
                  <div className="flex items-center gap-2 px-2 py-4 text-[12px] text-label-secondary">
                    <LoaderCircle className="h-4 w-4 animate-spin" />
                    Loading workspace…
                  </div>
                ) : null}
                {indexError ? (
                  <div className="px-2.5 py-2 rounded-[8px] bg-destructive/10 text-destructive">
                    <div className="text-[11px] font-semibold mb-0.5">Error</div>
                    <div className="text-[13px] leading-snug">{indexError}</div>
                  </div>
                ) : null}

                {filteredIndex ? (
                  <>
                    <div className="space-y-1">
                      <SidebarSectionHeading>Environments</SidebarSectionHeading>
                      {filteredIndex.environments.length === 0 ? (
                        <div className="px-2 py-1 text-[12px] text-label-tertiary">None</div>
                      ) : null}
                      {filteredIndex.environments.map((e) => (
                        <SidebarRowButton
                          key={e.name}
                          icon={<Layers className="h-4 w-4" strokeWidth={3} aria-hidden="true" />}
                          label={e.name}
                          subtitle={
                            <span className="inline-flex min-w-0 items-center gap-1 text-[11px]">
                              <span className="truncate">
                                base: {e.base} · promoted: {e.promoted_count}
                              </span>
                              {e.locked ? (
                                <span className="inline-flex items-center gap-1">
                                  <span aria-hidden="true">·</span>
                                  <LockKeyhole className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                                  <span>Locked</span>
                                </span>
                              ) : null}
                            </span>
                          }
                          trailing={
                            e.requires_approval ? (
                              <Sticker className="bg-[rgba(0,122,255,0.12)] text-primary">
                                <ShieldCheck className="h-3 w-3" strokeWidth={2.2} aria-hidden="true" />
                                {e.min_approvals}+
                              </Sticker>
                            ) : null
                          }
                          selected={selection.kind === "env" && selection.name === e.name}
                          onClick={() => requestDetails({ kind: "env", name: e.name })}
                        />
                      ))}
                    </div>

                    <div className="space-y-1">
                      <SidebarSectionHeading>Promoted branches</SidebarSectionHeading>
                      {filteredIndex.promoted_branches.length === 0 ? (
                        <div className="px-2 py-1 text-[12px] text-label-tertiary">None</div>
                      ) : null}
                      {filteredIndex.promoted_branches.map((b) => (
                        <SidebarRowButton
                          key={b.name}
                          icon={<ArrowUpRight className="h-4 w-4" strokeWidth={3} aria-hidden="true" />}
                          label={b.name}
                          subtitle={
                            b.promoted_to.length > 0
                              ? `Promoted to: ${b.promoted_to.join(", ")}`
                              : b.remote
                                ? "Remote"
                                : "Local"
                          }
                          trailing={
                            <Sticker>
                              {b.remote ? (
                                <>
                                  <Cloud className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                                  Remote
                                </>
                              ) : b.local ? (
                                <>
                                  <HardDrive className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                                  Local
                                </>
                              ) : (
                                "-"
                              )}
                            </Sticker>
                          }
                          selected={selection.kind === "branch" && selection.name === b.name}
                          onClick={() =>
                            requestDetails({ kind: "branch", name: b.name, isEnvironment: b.is_environment })
                          }
                        />
                      ))}
                    </div>

                    <div className="space-y-1">
                      <SidebarSectionHeading>Branches</SidebarSectionHeading>
                      {filteredIndex.branches.length === 0 ? (
                        <div className="px-2 py-1 text-[12px] text-label-tertiary">None</div>
                      ) : null}
                      {filteredIndex.branches.map((b) => (
                        <SidebarRowButton
                          key={b.name}
                          icon={<GitBranch className="h-4 w-4" aria-hidden="true" />}
                          label={b.name}
                          subtitle={
                            b.base_for.length > 0
                              ? `Base for: ${b.base_for.join(", ")}`
                              : b.remote
                                ? "Remote"
                                : "Local"
                          }
                          trailing={
                            <Sticker>
                              {b.remote ? (
                                <>
                                  <Cloud className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                                  Remote
                                </>
                              ) : b.local ? (
                                <>
                                  <HardDrive className="h-3 w-3" strokeWidth={2} aria-hidden="true" />
                                  Local
                                </>
                              ) : (
                                "-"
                              )}
                            </Sticker>
                          }
                          selected={selection.kind === "branch" && selection.name === b.name}
                          onClick={() =>
                            requestDetails({ kind: "branch", name: b.name, isEnvironment: b.is_environment })
                          }
                        />
                      ))}
                    </div>
                  </>
                ) : null}
              </div>
            </ScrollArea>
          </>
        )}
      </aside>

      <main className="relative flex min-w-0 flex-1 flex-col bg-background">
        <button
          type="button"
          aria-label="Close repository list"
          className={cn(
            "absolute inset-0 z-10 cursor-default transition-opacity duration-150 ease-out",
            repoListOpen
              ? "pointer-events-auto opacity-100 bg-black/20 backdrop-blur-[2px]"
              : "pointer-events-none opacity-0 bg-black/0 backdrop-blur-0"
          )}
          onClick={closeRepoList}
        />

        <div className="relative z-0 min-h-0 flex-1">
              <ScrollArea className="h-full" scrollbarClassName="bg-background">
                <div className="space-y-3 px-5 py-4">
                  {detailsError ? (
                    <div className="p-3 rounded-[8px] bg-destructive/10 text-destructive text-[13px]">
                      {detailsError}
                    </div>
                  ) : null}

                  {renderedSelection.kind === "none" ? (
                    <div className="flex flex-col items-center justify-center py-24 px-6">
                      <div className="text-[15px] font-medium text-label-tertiary text-center leading-relaxed">
                        Select a branch or environment to see details
                      </div>
                    </div>
                  ) : null}

                  {renderedSelection.kind !== "none" ? (
                    <div
                      key={detailsKey}
                      className="data-[state=open]:animate-in data-[state=open]:fade-in-0"
                      data-state="open"
                    >
                      <TabsContent value="overview" className="m-0 mt-0">
                        <OverviewPanel text={detailsOverview} dateFmt={dateFmt} />
                      </TabsContent>
                      <TabsContent value="timeline" className="m-0 mt-0">
                        {detailsTimeline.length === 0 ? (
                          <div className="py-24 flex items-center justify-center">
                            <div className="text-[13px] text-label-tertiary">No timeline entries</div>
                          </div>
	                        ) : (
	                          <div className="space-y-3">
	                            {detailsTimeline.map((t, i) => (
	                              <div key={i} className="space-y-1.5 pb-4 mb-3 hairline-b">
	                                  <div className="flex items-center justify-between gap-3 text-[11px] text-label-secondary">
	                                    <span className="min-w-0 truncate text-[11px] text-label-secondary">
	                                      {formatWhen(t.when, dateFmt)}
	                                    </span>
	                                    <Sticker className="shrink-0 self-center">
	                                      <TimelineKindIcon
	                                        kind={t.kind}
	                                        className="h-3 w-3"
	                                      /><span>{t.kind}</span>
	                                    </Sticker>	                                  </div>
	                                  <div className="text-[13px] font-medium text-label select-text">{t.summary}</div>
	                                  {t.detail ? (
	                                    <pre className="whitespace-pre-wrap break-words rounded-[8px] hairline bg-[var(--fill-soft)] p-3 text-[12px] font-mono leading-5 text-label-secondary select-text">
	                                      {t.detail}
	                                    </pre>
	                                  ) : null}

	                              </div>

                            ))}
                          </div>
                        )}
                      </TabsContent>
                    </div>
                  ) : null}
                </div>
              </ScrollArea>

              {detailsLoading ? (
                <div className="pointer-events-none absolute inset-0 flex items-start justify-end p-3">
                  <div className="flex items-center gap-2 rounded-[8px] hairline bg-popover px-3 py-1.5 text-[11px] text-label-secondary shadow-popover">
                    <LoaderCircle className="h-4 w-4 animate-spin text-label-secondary" aria-hidden="true" />
                    <div className="max-w-[28ch] truncate">
                      Loading{" "}
                      {selection.kind === "env"
                        ? selection.name
                        : selection.kind === "branch"
                          ? selection.name
                          : "details"}
                    </div>
                  </div>
                </div>
              ) : null}
            </div>
        </main>
      </div>
      </Tabs>

      {/* Repository missing */}
      <Dialog open={repoMissing != null} onOpenChange={(v) => (!v ? setRepoMissing(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <TriangleAlert className="h-4 w-4 text-warn" aria-hidden="true" />
              Repository not found
            </DialogTitle>
            <DialogDescription>Hitch Desktop can’t open this folder.</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <pre className="whitespace-pre-wrap break-words rounded-[6px] bg-[var(--fill-soft)] p-2.5 text-[12px] leading-5 text-label font-mono">
              {repoMissing?.repo.path}
            </pre>
            <div className="text-sm text-destructive">{repoMissing?.error}</div>
          </div>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button
              variant="destructive"
              onClick={() => (repoMissing ? removeRepo(repoMissing.repo.id) : undefined)}
            >
              <Trash2 className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
              Remove from list
            </Button>
            <Button variant="secondary" onClick={() => (repoMissing ? void locateRepo(repoMissing.repo) : undefined)}>
              <FolderSearch className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
              Locate…
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Locate mismatch */}
      <Dialog open={repoLocateMismatch != null} onOpenChange={(v) => (!v ? setRepoLocateMismatch(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <TriangleAlert className="h-4 w-4 text-warn" aria-hidden="true" />
              That doesn’t look like the same repository
            </DialogTitle>
            <DialogDescription>Identity mismatch. You can pick again or override.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <div className="text-[11px] font-medium text-label-secondary">Expected</div>
              <pre className="whitespace-pre-wrap break-words rounded-[6px] bg-[var(--fill-soft)] p-2.5 text-[12px] leading-5 text-label font-mono">
                {repoLocateMismatch ? expectedIdentityLabel(repoLocateMismatch.expected) : ""}
              </pre>
            </div>
            <div className="space-y-1">
              <div className="text-[11px] font-medium text-label-secondary">Found</div>
              <pre className="whitespace-pre-wrap break-words rounded-[6px] bg-[var(--fill-soft)] p-2.5 text-[12px] leading-5 text-label font-mono">
                {repoLocateMismatch ? expectedIdentityLabel(repoLocateMismatch.found) : ""}
              </pre>
            </div>
          </div>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button
              variant="destructive"
              onClick={() => {
                if (!repoLocateMismatch) return;
                const repo = repoLocateMismatch.repo;
                const next = repos.map((r) =>
                  r.id === repo.id
                    ? {
                        ...r,
                        path: repoLocateMismatch.foundPath,
                        expected_identity: repoLocateMismatch.found,
                        last_opened_at: nowIso()
                      }
                    : r
                );
                persist(next);
                setRepoLocateMismatch(null);
                setRepoMissing(null);
                setStatus("Repository updated");
              }}
            >
              <CheckCircle2 className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
              Use this repo anyway
            </Button>
            <Button
              variant="secondary"
              onClick={() => (repoLocateMismatch ? void locateRepo(repoLocateMismatch.repo) : undefined)}
            >
              <FolderSearch className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
              Pick another…
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Promote picker */}
      <Dialog open={promotePicker != null} onOpenChange={(v) => (!v ? setPromotePicker(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Promote {promotePicker?.branch}</DialogTitle>
            <DialogDescription>Choose an environment.</DialogDescription>
          </DialogHeader>
          <ScrollArea className="h-[45vh]">
            <div className="space-y-2 pr-3">
              {selectedRepo && index
                ? index.environments.map((e) => (
                    <Button
                      key={e.name}
                      variant="secondary"
                      className="w-full justify-start"
                      onClick={() => {
                        if (!promotePicker) return;
                        setPromotePicker(null);
                        void runOperation(`Promote ${promotePicker.branch} → ${e.name}`, (onLog) =>
                          promote(selectedRepo.path, promotePicker.branch, e.name, onLog)
                        );
                      }}
                    >
                      {e.name}
                    </Button>
                  ))
                : null}
            </div>
          </ScrollArea>
          <DialogFooter>
            <Button variant="ghost" onClick={() => setPromotePicker(null)}>
              Cancel
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Confirm dialog */}
      <Dialog open={confirm != null} onOpenChange={(v) => (!v ? setConfirm(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{confirm?.title}</DialogTitle>
            <DialogDescription>{confirm?.description}</DialogDescription>
          </DialogHeader>
          <DialogFooter className="gap-2 sm:gap-2">
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              Cancel
            </Button>
            <Button
              variant={confirm?.actionVariant ?? "default"}
              onClick={() => {
                const fn = confirm?.onAction;
                setConfirm(null);
                fn?.();
              }}
            >
              {confirm?.actionLabel}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Operation output */}
      <Dialog open={op != null} onOpenChange={(v) => (!v ? setOp(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{op?.title}</DialogTitle>
            <DialogDescription>
              {!op?.result ? "Running…" : op.result.ok ? "Done" : op.result.error ?? "Failed"}
            </DialogDescription>
          </DialogHeader>

          {(() => {
            const running = op != null && !op.result;
            const lines = op?.result ? op.result.lines : op?.lines ?? [];
            return (
              <ScrollArea className="h-[50vh]">
                <div className="space-y-2 pr-3">
                  {lines.map((l, i) => {
                    const levelClass =
                      l.level === "Info"
                        ? "text-label-secondary"
                        : l.level === "Warning"
                          ? "text-warn"
                          : l.level === "Error"
                            ? "text-destructive"
                            : "text-success";
                    return (
                      <div
                        key={i}
                        className="rounded-[8px] hairline bg-[var(--fill-soft)] px-3 py-2"
                      >
                        <div className={cn("flex items-center gap-2 text-[11px] font-medium", levelClass)}>
                          <OutputLevelIcon level={l.level} className={cn("h-3.5 w-3.5", levelClass)} />
                          <span>{l.level}</span>
                        </div>
                        <div className="mt-1 whitespace-pre-wrap break-words font-mono text-[12px] leading-5 text-label select-text">
                          {l.message}
                        </div>
                      </div>
                    );
                  })}
                  {running ? (
                    <div className="flex items-center gap-2 py-1 text-[12px] text-label-secondary">
                      <LoaderCircle className="h-4 w-4 animate-spin" aria-hidden="true" />
                      Running…
                    </div>
                  ) : lines.length === 0 ? (
                    <div className="text-[13px] text-label-secondary">No output.</div>
                  ) : null}
                </div>
              </ScrollArea>
            );
          })()}

          <DialogFooter>
            <Button
              variant="secondary"
              onClick={() => setOp(null)}
              disabled={op != null && !op.result}
            >
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
