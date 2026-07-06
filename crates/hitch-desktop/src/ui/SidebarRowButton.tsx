import React from "react";
import { cn } from "@/lib/utils";

type Props = Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon?: React.ReactNode;
  label: React.ReactNode;
  /** Small right-aligned count/text (e.g. promoted count). */
  meta?: React.ReactNode;
  /** Right-aligned node (lock icon, selection dot). */
  trailing?: React.ReactNode;
  selected?: boolean;
};

export function SidebarRowButton({
  icon,
  label,
  meta,
  trailing,
  selected,
  className,
  type,
  ...props
}: Props) {
  return (
    <button
      type={type ?? "button"}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-[6px] px-2 py-[5px] text-left transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        selected
          ? "bg-primary text-primary-foreground"
          : "text-label hover:bg-[var(--fill-soft)]",
        className
      )}
      {...props}
    >
      {icon ? (
        <span
          className={cn(
            "flex h-4 w-4 shrink-0 items-center justify-center",
            selected ? "text-primary-foreground/90" : "text-label-secondary"
          )}
          aria-hidden="true"
        >
          {icon}
        </span>
      ) : null}
      <span className="min-w-0 flex-1 truncate text-[13px] leading-tight tracking-tight">
        {label}
      </span>
      {meta != null ? (
        <span
          className={cn(
            "shrink-0 text-[11px] tabular-nums",
            selected ? "text-primary-foreground/80" : "text-label-tertiary"
          )}
        >
          {meta}
        </span>
      ) : null}
      {trailing ? (
        <span className="flex shrink-0 items-center self-center">{trailing}</span>
      ) : null}
    </button>
  );
}
