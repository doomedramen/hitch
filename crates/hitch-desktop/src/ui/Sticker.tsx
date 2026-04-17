import React from "react";
import { cn } from "@/lib/utils";

interface StickerProps {
  children: React.ReactNode;
  className?: string;
}

export function Sticker({ children, className }: StickerProps) {
  return (
    <span className={cn(
      "inline-flex items-center gap-1 rounded-none border-2 border-black bg-secondary px-2 py-0.5 text-[10px] font-black uppercase text-white shadow-neo-sm transition-transform duration-200 will-change-transform",
      className
    )}>
      {children}
    </span>
  );
}
