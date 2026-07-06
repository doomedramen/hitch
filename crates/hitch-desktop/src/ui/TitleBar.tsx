import { Info } from "lucide-react";
import React from "react";

export function TitleBar({ onAboutClick }: { onAboutClick: () => void }) {
  return (
    <div
      data-tauri-drag-region
      // Left padding clears the native macOS traffic lights (Overlay title bar).
      className="flex h-11 w-full shrink-0 items-center justify-between material-toolbar hairline-b pl-[78px] pr-2 select-none"
    >
      <span data-tauri-drag-region className="text-[13px] font-semibold tracking-tight text-label">
        Hitch Desktop
      </span>
      <button
        onClick={onAboutClick}
        title="About Hitch Desktop"
        aria-label="About Hitch Desktop"
        className="flex h-7 w-7 items-center justify-center rounded-[6px] text-label-secondary transition-colors hover:bg-[var(--fill-soft)] hover:text-label"
      >
        <Info className="h-[18px] w-[18px]" strokeWidth={1.8} aria-hidden="true" />
      </button>
    </div>
  );
}
