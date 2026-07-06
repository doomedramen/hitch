// Standalone visual preview of the redesigned sidebar + Overview.
// Renders the real components/CSS with illustrative mock data — no Tauri.
// Served by vite at /preview.html. NOT bundled into the shipped app.
import React from "react";
import { createRoot } from "react-dom/client";
import {
  ArrowUpRight,
  Check,
  ChevronDown,
  Clock,
  FileText,
  GitBranch,
  History,
  Info,
  Layers,
  LockKeyhole,
  RefreshCw,
  Rocket,
  Search,
  ShieldCheck
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { SidebarRowButton } from "./ui/SidebarRowButton";
import "./globals.css";

function Pill({ tone, children }: { tone: "lock" | "approval"; children: React.ReactNode }) {
  const toneCls =
    tone === "lock" ? "bg-[var(--warn-soft)] text-warn" : "bg-[rgba(0,122,255,0.12)] text-primary";
  return (
    <span className={cn("inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium", toneCls)}>
      {children}
    </span>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return <div className="mb-2 mt-6 text-[12px] font-semibold text-label-secondary">{children}</div>;
}

function KvRow({ k, v, mono, first }: { k: string; v: React.ReactNode; mono?: boolean; first?: boolean }) {
  return (
    <div
      className={cn(
        "grid grid-cols-[150px_minmax(0,1fr)] gap-3.5 px-3.5 py-2.5 text-[13px] items-center",
        first ? "" : "hairline-t"
      )}
    >
      <div className="text-label-secondary">{k}</div>
      <div className={cn("min-w-0 text-label break-words", mono ? "font-mono text-[12px] text-label-secondary" : "")}>{v}</div>
    </div>
  );
}

function SidebarHeading({ children }: { children: React.ReactNode }) {
  return <div className="px-2 pt-3 pb-1 text-[11px] font-semibold text-label-secondary">{children}</div>;
}

function Toolbar() {
  return (
    <header className="flex h-12 shrink-0 items-center gap-2.5 material-toolbar hairline-b pl-[82px] pr-2">
      <button className="flex h-7 min-w-0 max-w-[240px] items-center gap-2 rounded-[6px] bg-[var(--control-bg)] px-2 text-left shadow-control">
        <FileText className="h-4 w-4 shrink-0 text-label-secondary" strokeWidth={1.8} />
        <span className="truncate text-[13px] font-medium tracking-tight text-label">qab</span>
        <ChevronDown className="h-4 w-4 shrink-0 text-label-secondary" />
      </button>
      <div className="inline-flex rounded-[7px] bg-[var(--seg-bg)] p-0.5">
        <button className="inline-flex items-center gap-1.5 rounded-[5px] bg-[var(--control-bg)] px-3 py-1 text-[13px] font-medium text-label shadow-control">
          <FileText className="h-3.5 w-3.5" strokeWidth={2} /> Overview
        </button>
        <button className="inline-flex items-center gap-1.5 rounded-[5px] px-3 py-1 text-[13px] font-medium text-label">
          <History className="h-3.5 w-3.5" strokeWidth={2} /> Timeline
        </button>
      </div>
      <div className="flex-1" />
      <Button variant="secondary" size="sm" className="h-7 justify-center">
        <ShieldCheck className="h-4 w-4" strokeWidth={2} />
        Approvals
        <span className="ml-0.5 inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-destructive px-1 text-[10px] font-semibold text-destructive-foreground">
          1
        </span>
      </Button>
      <Button variant="secondary" size="sm" className="h-7 justify-center">
        <RefreshCw className="h-4 w-4" strokeWidth={2} />
        <span className="hidden md:inline">Rebuild</span>
      </Button>
      <Button variant="destructive" size="sm" className="h-7 justify-center">
        <Rocket className="h-4 w-4" strokeWidth={2} />
        <span className="hidden md:inline">Release</span>
      </Button>
      <button className="ml-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-[6px] text-label-secondary hover:bg-[var(--fill-soft)]">
        <Info className="h-[18px] w-[18px]" strokeWidth={1.8} />
      </button>
    </header>
  );
}

function Sidebar() {
  const [sel, setSel] = React.useState("qa");
  const branches = [
    { name: "fix-monthly-flag", base: false },
    { name: "main", base: true },
    { name: "test/about-you-cold-entry", base: false },
    { name: "test/contract-migration", base: false },
    { name: "test/fix-nightly-run", base: false }
  ];
  return (
    <aside className="flex w-[244px] shrink-0 flex-col hairline-r material-sidebar">
      <div className="px-2.5 py-2.5">
        <div className="flex min-w-0 items-center gap-2 rounded-[7px] bg-[var(--fill-soft)] px-2.5 py-[6px] text-label-tertiary">
          <Search className="h-3.5 w-3.5 shrink-0" strokeWidth={2} />
          <span className="flex-1 text-[13px] text-label-tertiary">Filter</span>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        <SidebarHeading>Environments</SidebarHeading>
        <SidebarRowButton
          icon={<Layers className="h-4 w-4" strokeWidth={2} />}
          label="dev"
          meta={2}
          selected={sel === "dev"}
          onClick={() => setSel("dev")}
        />
        <SidebarRowButton
          icon={<Layers className="h-4 w-4" strokeWidth={2} />}
          label="qa"
          trailing={
            <LockKeyhole
              className={cn("h-3.5 w-3.5", sel === "qa" ? "text-primary-foreground/90" : "text-warn")}
              strokeWidth={2}
            />
          }
          selected={sel === "qa"}
          onClick={() => setSel("qa")}
        />

        <SidebarHeading>Promoted branches</SidebarHeading>
        <SidebarRowButton
          icon={<ArrowUpRight className="h-4 w-4" strokeWidth={2} />}
          label="feature/checkout"
          selected={sel === "feature/checkout"}
          onClick={() => setSel("feature/checkout")}
        />

        <SidebarHeading>Branches</SidebarHeading>
        {branches.map((b) => (
          <SidebarRowButton
            key={b.name}
            icon={<GitBranch className="h-4 w-4" strokeWidth={2} />}
            label={b.name}
            trailing={
              b.base ? (
                <span
                  className={cn(
                    "text-[10px] font-medium uppercase tracking-wide",
                    sel === b.name ? "text-primary-foreground/70" : "text-label-tertiary"
                  )}
                >
                  base
                </span>
              ) : null
            }
            selected={sel === b.name}
            onClick={() => setSel(b.name)}
          />
        ))}
      </div>
    </aside>
  );
}

function ApprovalCard() {
  const approvers = [
    { email: "alice@acme.com", done: true },
    { email: "carol@acme.com", done: false },
    { email: "ben@acme.com", done: false, requester: true }
  ];
  return (
    <div className="rounded-[10px] hairline-strong bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-1.5 text-[14px] font-semibold text-label">
            <ArrowUpRight className="h-3.5 w-3.5 shrink-0 text-label-secondary" strokeWidth={2} />
            <span className="min-w-0 truncate">feature/payments → qa</span>
          </div>
          <div className="mt-1 text-[12px] text-label-secondary">Promote · requested by ben@acme.com · 2 hours ago</div>
        </div>
        <Pill tone="lock">
          <Clock className="h-3 w-3" strokeWidth={2} />
          Pending
        </Pill>
      </div>
      <div className="my-3.5 flex items-center gap-2.5">
        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--fill-soft)]">
          <div className="h-full rounded-full bg-primary" style={{ width: "50%" }} />
        </div>
        <span className="shrink-0 text-[12px] font-semibold tabular-nums text-label-secondary">1 / 2</span>
      </div>
      <div className="flex flex-wrap gap-1.5">
        {approvers.map((a) => (
          <span
            key={a.email}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-full px-2 py-[3px] text-[11px]",
              a.done ? "bg-[var(--success-soft)] text-success" : "hairline-strong text-label-secondary"
            )}
          >
            {a.done ? <Check className="h-3 w-3" strokeWidth={2.4} /> : <Clock className="h-3 w-3" strokeWidth={1.8} />}
            {a.email}
            {a.requester ? " · requester" : ""}
          </span>
        ))}
      </div>
      <div className="mt-4 flex gap-2">
        <Button size="sm" className="h-6 justify-center px-2.5 text-[12px]">
          <ShieldCheck className="h-3.5 w-3.5" strokeWidth={2} />
          Approve
        </Button>
        <Button variant="secondary" size="sm" className="h-6 justify-center px-2.5 text-[12px] text-destructive">
          Reject
        </Button>
      </div>
    </div>
  );
}

function Content() {
  return (
    <main className="min-w-0 flex-1 overflow-y-auto bg-background px-5 py-4">
      <div className="mb-5 select-text">
        <div className="flex flex-wrap items-center gap-2.5">
          <h1 className="m-0 text-[20px] font-semibold tracking-[-0.01em] text-label">qa</h1>
          <Pill tone="lock">
            <LockKeyhole className="h-3 w-3" strokeWidth={2} />
            Locked
          </Pill>
          <Pill tone="approval">Requires 2 approvals</Pill>
        </div>
        <p className="mb-0 mt-1.5 text-[13px] text-label-secondary">
          Environment branch built from main plus 1 promoted branch.
        </p>
      </div>

      <SectionTitle>Overview</SectionTitle>
      <div className="overflow-hidden rounded-[8px] hairline-strong">
        <KvRow first k="Base branch" v="main" />
        <KvRow k="Promoted" v="feature/checkout" />
        <KvRow k="Head" v="a1f9c30" mono />
        <KvRow k="Locked by" v="wookoouk@gmail.com" />
        <KvRow k="Rebuilt" v="4 hours ago" />
      </div>

      <SectionTitle>Pending approval</SectionTitle>
      <ApprovalCard />
    </main>
  );
}

function PreviewApp() {
  return (
    <div className="min-h-screen w-full p-8" style={{ background: "radial-gradient(120% 120% at 20% 0%, #b7c7e6 0%, #93a7d0 42%, #6f83b4 100%)" }}>
      <div className="mx-auto flex w-full max-w-[1040px] flex-col overflow-hidden rounded-[11px] border border-black/10 shadow-2xl" style={{ height: 620 }}>
        <div className="flex min-h-0 flex-1 flex-col text-label">
          <Toolbar />
          <div className="flex min-h-0 flex-1">
            <Sidebar />
            <Content />
          </div>
        </div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <PreviewApp />
  </React.StrictMode>
);
