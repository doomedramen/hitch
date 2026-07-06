import { Info } from "lucide-react";
import React from "react";

export function TitleBar({ onAboutClick }: { onAboutClick: () => void }) {
  return (
    <div
      data-tauri-drag-region
      // 28px = the standard macOS title-bar height, so the native traffic lights
      // (Overlay style) sit vertically centered in it. Left padding clears them.
      className="flex h-7 w-full shrink-0 items-center justify-between material-toolbar hairline-b pl-[78px] pr-1.5 select-none"
    >
      <span data-tauri-drag-region className="text-[13px] font-semibold tracking-tight text-label">
        Hitch Desktop
      </span>
      <button
        onClick={onAboutClick}
        title="About Hitch Desktop"
        aria-label="About Hitch Desktop"
        className="flex h-5 w-5 items-center justify-center rounded-[5px] text-label-secondary transition-colors hover:bg-[var(--fill-soft)] hover:text-label"
      >
        <Info className="h-4 w-4" strokeWidth={1.8} aria-hidden="true" />
      </button>
    </div>
  );
}
