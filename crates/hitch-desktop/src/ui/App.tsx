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
  Layers,
  LoaderCircle,
  LockKeyhole,
  Monitor,
  RefreshCw,
  Rocket,
  Search,
  ShieldCheck,
  Trash2,
  TriangleAlert
} from "lucide-react";
import { loadRepos, saveRepos } from "./storage";
import type {
  BranchDetailsModel,
  EnvironmentDetailsModel,
  OperationResult,
  RepoEntry,
  RepoIdentity,
  WorkspaceIndexModel
} from "./types";
import { branchDetails, envDetails, promote, rebuild, release, repoProbe, workspaceIndex } from "./tauri";
import { OutputLevelIcon, RepoIdentityIcon, TimelineKindIcon } from "./icons";
import { SidebarRowButton } from "./SidebarRowButton";

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
        "px-2 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground",
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

function OverviewPanel({ text }: { text: string }) {
  const rows = useMemo(() => parseOverview(text), [text]);
  if (rows.length === 0) return <div className="text-sm text-muted-foreground">No overview.</div>;

  return (
    <div className="space-y-1">
      {rows.map((row, idx) => {
        const showDivider = idx !== rows.length - 1;
        if (row.kind === "text") {
          return (
            <div key={idx} className={cn("py-2 text-sm", showDivider ? "border-b border-border" : "")}>
              {row.value}
            </div>
          );
        }

        if (row.kind === "list") {
          return (
            <div key={idx} className={cn("py-2", showDivider ? "border-b border-border" : "")}>
              <div className="grid grid-cols-[140px_1fr] gap-3">
                <div className="pt-0.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">
                  {row.key}
                </div>
                <div className="min-w-0">
                  {row.items.length === 0 ? (
                    <div className="text-sm text-muted-foreground">None</div>
                  ) : (
                    <ul className="space-y-1">
                      {row.items.map((it, j) => (
                        <li key={j} className="flex gap-2 text-sm">
                          <span className="mt-2 h-1 w-1 shrink-0 rounded-full bg-muted-foreground/60" />
                          <span className={cn("min-w-0 break-words", looksCodey(it) ? "font-mono text-xs" : "")}>{it}</span>
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
                "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium",
                isYes ? "bg-destructive/15 text-destructive" : "bg-secondary text-secondary-foreground"
              )}
            >
              {row.value}
            </span>
          ) : null;

        return (
          <div key={idx} className={cn("py-2", showDivider ? "border-b border-border" : "")}>
            <div className="grid grid-cols-[140px_1fr] gap-3">
              <div className="pt-0.5 text-xs font-medium uppercase tracking-wide text-muted-foreground">{row.key}</div>
              <div className="min-w-0 text-sm">
                {badge ?? (
                  <span className={cn("break-words", looksCodey(row.value) ? "font-mono text-xs" : "")}>
                    {row.value.length === 0 ? <span className="text-muted-foreground">—</span> : row.value}
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

export function App() {
  const [repos, setRepos] = useState<RepoEntry[]>([]);
  const [selectedRepoId, setSelectedRepoId] = useState<string | null>(null);
  const [repoListOpen, setRepoListOpen] = useState<boolean>(false);
  const [repoFilter, setRepoFilter] = useState<string>("");

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

  const [op, setOp] = useState<null | { title: string; result?: OperationResult }>(null);
  const [promotePicker, setPromotePicker] = useState<null | { branch: string }>(null);
  const [confirm, setConfirm] = useState<ConfirmState>(null);

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

  async function runOperation(title: string, opFn: () => Promise<OperationResult>) {
    setOp({ title });
    setStatus(title);
    try {
      const result = await opFn();
      setOp({ title, result });
      setStatus(result.ok ? "Done" : "Failed");
      // refresh index
      if (selectedRepo) {
        try {
          const idx = await workspaceIndex(selectedRepo.path);
          setIndex(idx);
          if (result.ok) setStatus("Ready");
        } catch {
          // ignore
        }
      }
    } catch (e: any) {
      setOp({ title, result: { ok: false, error: e?.toString?.() ?? "Operation failed", lines: [] } });
      setStatus("Failed");
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
    <div className="flex h-full w-full overflow-hidden">
      <aside className="flex w-[420px] min-w-[340px] shrink-0 flex-col border-r border-border bg-secondary">
        <div className="border-b border-border/70 bg-gradient-to-b from-black/10 to-secondary px-3 pb-3 pt-3">
          <button
            type="button"
            onClick={toggleRepoList}
            className={cn(
              "flex w-full items-center justify-between gap-3 rounded-md px-2 py-2 text-left transition-colors",
              "hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background"
            )}
          >
            <div className="flex min-w-0 items-center gap-3">
              <Monitor className="h-5 w-5 shrink-0 text-muted-foreground" aria-hidden="true" />
              <div className="text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
                Current Repository
              </div>
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold tracking-tight">
                  {selectedRepo ? selectedRepo.display_name : "Select a repository"}
                </div>
              </div>
            </div>
            {repoListOpen ? (
              <ChevronUp className="h-5 w-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            ) : (
              <ChevronDown className="h-5 w-5 shrink-0 text-muted-foreground" aria-hidden="true" />
            )}
          </button>
        </div>

        {repoListOpen ? (
          <>
            <div className="flex items-center gap-2 px-3 py-3">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
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
                className="h-9 shrink-0 min-w-20 justify-center gap-2"
                onClick={() => {
                  closeRepoList();
                  void addRepo();
                }}
              >
                Add
                <ChevronDown className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
              </Button>
            </div>
            <Separator />
            <ScrollArea className="flex-1" viewportClassName="pr-3">
              <div className="space-y-6 p-2">
                {repos.length === 0 ? (
                  <div className="px-2 py-1 text-sm text-muted-foreground">No repositories yet.</div>
                ) : repoListModel.mode === "filtered" ? (
                  <div className="space-y-1">
                    <SidebarSectionHeading>Results</SidebarSectionHeading>
                    {repoListModel.results.length === 0 ? (
                      <div className="px-2 py-1 text-sm text-muted-foreground">No matches.</div>
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
                        <div className="px-2 py-1 text-sm text-muted-foreground">No recent repositories.</div>
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
                <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
                <Input
                  placeholder="Filter"
                  className="pl-9"
                  value={filter}
                  onChange={(e) => setFilter(e.target.value)}
                />
              </div>

              {indexLoading ? <div className="text-xs text-muted-foreground">Loading…</div> : null}
              {indexError ? <div className="text-xs text-destructive">{indexError}</div> : null}
              {!filteredIndex && !indexLoading && !indexError && !selectedRepo ? (
                <div className="text-xs text-muted-foreground">No workspace loaded.</div>
              ) : null}
            </div>
            <Separator />

            <ScrollArea className="flex-1" viewportClassName="pr-3">
              <div className="space-y-6 p-2">
                {filteredIndex ? (
                  <>
                    <div className="space-y-1">
                      <SidebarSectionHeading>Environments</SidebarSectionHeading>
                      {filteredIndex.environments.length === 0 ? (
                        <div className="px-2 py-1 text-sm text-muted-foreground">None</div>
                      ) : null}
                      {filteredIndex.environments.map((e) => (
                        <SidebarRowButton
                          key={e.name}
                          icon={<Layers className="h-4 w-4" aria-hidden="true" />}
                          label={e.name}
                          subtitle={
                            <span className="inline-flex min-w-0 items-center gap-1">
                              <span className="truncate">
                                base: {e.base} • promoted: {e.promoted_count}
                              </span>
                              {e.locked ? (
                                <span className="inline-flex items-center gap-1">
                                  <span aria-hidden="true">•</span>
                                  <LockKeyhole className="h-3.5 w-3.5" aria-hidden="true" />
                                  <span>locked</span>
                                </span>
                              ) : null}
                            </span>
                          }
                          trailing={
                            <span className="inline-flex items-center gap-1 rounded-full bg-secondary px-2 py-0.5 text-xs text-secondary-foreground">
                              {e.requires_approval ? (
                                <>
                                  <ShieldCheck className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                                  approvals {e.min_approvals}+
                                </>
                              ) : (
                                "open"
                              )}
                            </span>
                          }
                          selected={selection.kind === "env" && selection.name === e.name}
                          onClick={() => requestDetails({ kind: "env", name: e.name })}
                        />
                      ))}
                    </div>

                    <div className="space-y-1">
                      <SidebarSectionHeading>Promoted branches</SidebarSectionHeading>
                      {filteredIndex.promoted_branches.length === 0 ? (
                        <div className="px-2 py-1 text-sm text-muted-foreground">None</div>
                      ) : null}
                      {filteredIndex.promoted_branches.map((b) => (
                        <SidebarRowButton
                          key={b.name}
                          icon={<ArrowUpRight className="h-4 w-4" aria-hidden="true" />}
                          label={b.name}
                          subtitle={
                            b.promoted_to.length > 0
                              ? `promoted to: ${b.promoted_to.join(", ")}`
                              : b.remote
                                ? "remote"
                                : "local"
                          }
                          trailing={
                            <span className="inline-flex items-center gap-1 rounded-full bg-secondary px-2 py-0.5 text-xs text-secondary-foreground">
                              {b.remote ? (
                                <>
                                  <Cloud className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                                  remote
                                </>
                              ) : b.local ? (
                                <>
                                  <HardDrive className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                                  local
                                </>
                              ) : (
                                "-"
                              )}
                            </span>
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
                        <div className="px-2 py-1 text-sm text-muted-foreground">None</div>
                      ) : null}
                      {filteredIndex.branches.map((b) => (
                        <SidebarRowButton
                          key={b.name}
                          icon={<GitBranch className="h-4 w-4" aria-hidden="true" />}
                          label={b.name}
                          subtitle={
                            b.base_for.length > 0
                              ? `base for: ${b.base_for.join(", ")}`
                              : b.remote
                                ? "remote"
                                : "local"
                          }
                          trailing={
                            <span className="inline-flex items-center gap-1 rounded-full bg-secondary px-2 py-0.5 text-xs text-secondary-foreground">
                              {b.remote ? (
                                <>
                                  <Cloud className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                                  remote
                                </>
                              ) : b.local ? (
                                <>
                                  <HardDrive className="h-3.5 w-3.5 text-muted-foreground" aria-hidden="true" />
                                  local
                                </>
                              ) : (
                                "-"
                              )}
                            </span>
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
              ? "pointer-events-auto opacity-100 bg-black/35 backdrop-blur-sm backdrop-saturate-150"
              : "pointer-events-none opacity-0 bg-black/0 backdrop-blur-0"
          )}
          onClick={closeRepoList}
        />

        <section className="relative z-0 flex min-h-0 flex-1 flex-col">
          <Tabs
            value={tab}
            onValueChange={(v) => setTab(v === "timeline" ? "timeline" : "overview")}
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="flex items-center justify-between gap-3 px-3 py-3">
              <TabsList>
                <TabsTrigger value="overview" className="group">
                  <FileText className="mr-2 h-4 w-4 text-muted-foreground group-data-[state=active]:text-foreground/80" aria-hidden="true" />
                  Overview
                </TabsTrigger>
                <TabsTrigger value="timeline" className="group">
                  <History className="mr-2 h-4 w-4 text-muted-foreground group-data-[state=active]:text-foreground/80" aria-hidden="true" />
                  Timeline
                </TabsTrigger>
              </TabsList>

              <div className="flex items-center gap-3">
                {status !== "Ready" ? (
                  <div className="hidden text-xs text-muted-foreground sm:block">{status}</div>
                ) : null}
                {selectedRepo && selection.kind === "branch" && !selection.isEnvironment ? (
                  <Button
                    variant="outline"
                    size="sm"
                    className="min-w-24 justify-center"
                    onClick={() => setPromotePicker({ branch: selection.name })}
                  >
                    <ArrowUpRight className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
                    Promote…
                  </Button>
                ) : null}
                {selectedRepo && selection.kind === "env" ? (
                  <>
                    <Button
                      variant="outline"
                      size="sm"
                      className="min-w-24 justify-center"
                      onClick={() =>
                        setConfirm({
                          title: `Rebuild ${selection.name}`,
                          description: `Rebuild environment '${selection.name}'?`,
                          actionLabel: "Rebuild",
                          actionVariant: "outline",
                          onAction: () =>
                            void runOperation(`Rebuild ${selection.name}`, () =>
                              rebuild(selectedRepo.path, selection.name, false)
                            )
                        })
                      }
                    >
                      <RefreshCw className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
                      Rebuild
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      className="min-w-24 justify-center"
                      onClick={() =>
                        setConfirm({
                          title: `Release ${selection.name}`,
                          description: `Release environment '${selection.name}'? This can rewrite history of the target branch.`,
                          actionLabel: "Release",
                          actionVariant: "destructive",
                          onAction: () =>
                            void runOperation(`Release ${selection.name}`, () => release(selectedRepo.path, selection.name))
                        })
                      }
                    >
                      <Rocket className="mr-2 h-4 w-4 opacity-90" aria-hidden="true" />
                      Release
                    </Button>
                  </>
                ) : null}
              </div>
            </div>
            <Separator />

            <div className="relative min-h-0 flex-1">
              <ScrollArea className="h-full">
                <div className="space-y-3 px-4 py-4">
                  {detailsError ? <div className="text-sm text-destructive">{detailsError}</div> : null}
                  {renderedSelection.kind === "none" ? (
                    <div className="text-sm text-muted-foreground">
                      Select a branch or environment to see details.
                    </div>
                  ) : null}

                  {renderedSelection.kind !== "none" ? (
                    <div
                      key={detailsKey}
                      className="data-[state=open]:animate-in data-[state=open]:fade-in-0"
                      data-state="open"
                    >
                      <TabsContent value="overview" className="m-0 mt-0">
                        <OverviewPanel text={detailsOverview} />
                      </TabsContent>
                      <TabsContent value="timeline" className="m-0 mt-0">
                        {detailsTimeline.length === 0 ? (
                          <div className="text-sm text-muted-foreground">No timeline entries.</div>
                        ) : (
                          <div className="space-y-3">
                            {detailsTimeline.map((t, i) => (
                              <div key={i} className="space-y-1">
                                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                  <span>{dateFmt.format(new Date(t.when))}</span>
                                  <span aria-hidden="true">•</span>
                                  <span className="inline-flex items-center gap-1">
                                    <TimelineKindIcon kind={t.kind} className="h-3.5 w-3.5" />
                                    <span>{t.kind}</span>
                                  </span>
                                </div>
                                <div className="text-sm">{t.summary}</div>
                                {t.detail ? (
                                  <pre className="whitespace-pre-wrap break-words rounded-md bg-secondary/40 p-3 text-xs leading-5 text-foreground">
                                    {t.detail}
                                  </pre>
                                ) : null}
                                <Separator className="mt-3" />
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
                  <div className="flex items-center gap-2 rounded-md border border-border bg-background/80 px-2 py-1 text-xs text-muted-foreground backdrop-blur">
                    <LoaderCircle className="h-4 w-4 animate-spin text-muted-foreground" aria-hidden="true" />
                    <div className="max-w-[28ch] truncate">
                      Loading{" "}
                      {selection.kind === "env"
                        ? selection.name
                        : selection.kind === "branch"
                          ? selection.name
                          : "details"}
                      …
                    </div>
                  </div>
                </div>
              ) : null}
            </div>
          </Tabs>
        </section>
      </main>

      {/* Repository missing */}
      <Dialog open={repoMissing != null} onOpenChange={(v) => (!v ? setRepoMissing(null) : null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <TriangleAlert className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
              Repository not found
            </DialogTitle>
            <DialogDescription>Hitch Desktop can’t open this folder.</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <pre className="whitespace-pre-wrap break-words rounded-md bg-secondary/40 p-3 text-xs leading-5 text-foreground">
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
              <TriangleAlert className="h-4 w-4 text-muted-foreground" aria-hidden="true" />
              That doesn’t look like the same repository
            </DialogTitle>
            <DialogDescription>Identity mismatch. You can pick again or override.</DialogDescription>
          </DialogHeader>
          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <div className="text-xs font-medium text-muted-foreground">Expected</div>
              <pre className="whitespace-pre-wrap break-words rounded-md bg-secondary/40 p-3 text-xs leading-5 text-foreground">
                {repoLocateMismatch ? expectedIdentityLabel(repoLocateMismatch.expected) : ""}
              </pre>
            </div>
            <div className="space-y-1">
              <div className="text-xs font-medium text-muted-foreground">Found</div>
              <pre className="whitespace-pre-wrap break-words rounded-md bg-secondary/40 p-3 text-xs leading-5 text-foreground">
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
                        void runOperation(`Promote ${promotePicker.branch} → ${e.name}`, () =>
                          promote(selectedRepo.path, promotePicker.branch, e.name)
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

          {op?.result ? (
            <ScrollArea className="h-[50vh]">
              <div className="space-y-2 pr-3">
                {op.result.lines.length === 0 ? (
                  <div className="text-sm text-muted-foreground">No output.</div>
                ) : (
                  op.result.lines.map((l, i) => {
                    const levelClass =
                      l.level === "Info"
                        ? "text-muted-foreground"
                        : l.level === "Warning"
                          ? "text-amber-300"
                          : l.level === "Error"
                            ? "text-destructive"
                            : "text-primary";
                    return (
                      <div
                        key={i}
                        className="rounded-md border border-border bg-secondary/30 px-3 py-2"
                      >
                        <div className={cn("flex items-center gap-2 text-xs font-medium", levelClass)}>
                          <OutputLevelIcon level={l.level} className={cn("h-3.5 w-3.5", levelClass)} />
                          <span>{l.level}</span>
                        </div>
                        <div className="mt-1 whitespace-pre-wrap break-words font-mono text-xs leading-5 text-foreground">
                          {l.message}
                        </div>
                      </div>
                    );
                  })
                )}
              </div>
            </ScrollArea>
          ) : (
            <div className="text-sm text-muted-foreground">Running…</div>
          )}

          <DialogFooter>
            <Button variant="secondary" onClick={() => setOp(null)} disabled={!op}>
              Close
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
