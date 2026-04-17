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
        "grid w-full grid-cols-[auto_minmax(0,1fr)_auto] gap-x-3 rounded-none border-2 border-transparent pl-2 pr-3 text-left transition-all",
        "focus-visible:outline-none focus-visible:translate-x-[2px] focus-visible:border-black",
        hasSubtitle ? "items-start py-2" : "h-11 items-center",
        selected
          ? "border-black bg-primary text-primary-foreground shadow-neo-sm"
          : "hover:border-black hover:bg-primary/80",
        className
      )}
      {...props}
    >
      {icon ? (
        <span
          className={cn(
            "shrink-0",
            selected ? "text-inherit" : "text-black/60",
            hasSubtitle ? "mt-0.5" : ""
          )}
          aria-hidden="true"
        >
          {icon}
        </span>
      ) : null}
      <span className="min-w-0 overflow-hidden">
        <span className="block truncate text-sm font-black uppercase tracking-tight">{label}</span>
        {hasSubtitle ? <span className="mt-0.5 block truncate text-[10px] font-bold uppercase text-black/60">{subtitle}</span> : null}
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
