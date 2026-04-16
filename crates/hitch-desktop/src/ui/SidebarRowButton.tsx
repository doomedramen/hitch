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
        "grid w-full grid-cols-[auto_minmax(0,1fr)_auto] gap-x-3 rounded-md pl-2 pr-3 text-left transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 ring-offset-background",
        hasSubtitle ? "items-start py-2" : "h-10 items-center",
        selected ? "bg-accent text-accent-foreground" : "hover:bg-accent/60",
        className
      )}
      {...props}
    >
      {icon ? (
        <span
          className={cn(
            "shrink-0",
            selected ? "text-accent-foreground/80" : "text-muted-foreground",
            hasSubtitle ? "mt-0.5" : ""
          )}
          aria-hidden="true"
        >
          {icon}
        </span>
      ) : null}
      <span className="min-w-0 overflow-hidden">
        <span className="block truncate text-sm">{label}</span>
        {hasSubtitle ? <span className="mt-0.5 block truncate text-xs text-muted-foreground">{subtitle}</span> : null}
      </span>
      {trailing ? (
        <span
          className={cn(
            "shrink-0 justify-self-end",
            hasSubtitle ? "pt-0.5" : "self-center"
          )}
        >
          {trailing}
        </span>
      ) : null}
    </button>
  );
}
