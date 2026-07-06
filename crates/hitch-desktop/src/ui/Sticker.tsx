import React from "react";
import { cn } from "@/lib/utils";

interface StickerProps {
  children: React.ReactNode;
  className?: string;
}

export function Sticker({ children, className }: StickerProps) {
  return (
    <span className={cn(
      "inline-flex items-center gap-1 rounded-full bg-[var(--fill-soft)] px-2 py-0.5 text-[11px] font-medium text-label-secondary",
      className
    )}>
      {children}
    </span>
  );
}
