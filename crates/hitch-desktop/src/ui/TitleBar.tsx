import { getCurrentWindow } from "@tauri-apps/api/window";
import { Info, Maximize2, Minus as MinusIcon, X } from "lucide-react";
import React from "react";
import { HitchIcon } from "./index";

const appWindow = getCurrentWindow();

export function TitleBar({ onAboutClick }: { onAboutClick: () => void }) {
  return (
    <div
      data-tauri-drag-region
      className="flex h-11 w-full items-center justify-between border-b-4 border-black bg-primary pl-3 select-none"
    >
      <div data-tauri-drag-region className="flex items-center gap-2">
        <button
          onClick={onAboutClick}
          className="hover:translate-x-[-1px] hover:translate-y-[-1px] hover:shadow-neo transition-all active:translate-x-[0px] active:translate-y-[0px] active:shadow-none"
        >
          <HitchIcon size="sm" />
        </button>
        <span className="text-xs font-black uppercase tracking-widest text-black">Hitch Desktop</span>
      </div>
      <div className="flex h-full items-center">
        <button
          onClick={onAboutClick}
          className="flex h-full w-11 items-center justify-center border-l-2 border-black hover:bg-white hover:text-black transition-colors"
          title="About"
        >
          <Info className="h-4 w-4" strokeWidth={3} />
        </button>
        <button
          onClick={() => void appWindow.minimize()}
          className="flex h-full w-11 items-center justify-center border-l-2 border-black hover:bg-[#FFB000] hover:text-black transition-colors"
          title="Minimize"
        >
          <MinusIcon className="h-4 w-4" strokeWidth={3} />
        </button>
        <button
          onClick={() => void appWindow.toggleMaximize()}
          className="flex h-full w-11 items-center justify-center border-l-2 border-black hover:bg-accent hover:text-black transition-colors"
          title="Maximize"
        >
          <Maximize2 className="h-4 w-4" strokeWidth={3} />
        </button>
        <button
          onClick={() => void appWindow.close()}
          className="flex h-full w-12 items-center justify-center border-l-2 border-black hover:bg-destructive hover:text-white transition-colors"
          title="Close"
        >
          <X className="h-4 w-4" strokeWidth={3} />
        </button>
      </div>
    </div>
  );
}
