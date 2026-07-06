import React from "react";
import { cn } from "@/lib/utils";

type Props = Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> & {
  icon?: React.ReactNode;
  label: React.ReactNode;
  subtitle?: React.ReactNode;
  trailing?: React.ReactNode;
  selected?: boolean;
};

export function SidebarRowButton({
  icon,
  label,
  subtitle,
  trailing,
  selected,
  className,
  type,
  ...props
}: Props) {
  const hasSubtitle = subtitle != null;
  return (
    <button
      type={type ?? "button"}
      className={cn(
        "grid w-full grid-cols-[auto_minmax(0,1fr)_auto] gap-x-2.5 rounded-[6px] px-2 text-left transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
        hasSubtitle ? "items-start py-1.5" : "h-8 items-center",
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
            "shrink-0 self-center",
            selected ? "text-primary-foreground/85" : "text-label-secondary"
          )}
          aria-hidden="true"
        >
          {icon}
        </span>
      ) : null}
      <span className="min-w-0 overflow-hidden">
        <span className="block truncate text-[13px] font-normal tracking-tight">{label}</span>
        {hasSubtitle ? (
          <span
            className={cn(
              "mt-0.5 block truncate text-[11px]",
              selected ? "text-primary-foreground/75" : "text-label-tertiary"
            )}
          >
            {subtitle}
          </span>
        ) : null}
      </span>
      {trailing ? (
        <span className="shrink-0 justify-self-end self-center">
          {trailing}
        </span>
      ) : null}
    </button>
  );
}
