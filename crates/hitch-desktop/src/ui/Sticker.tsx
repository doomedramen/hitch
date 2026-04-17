import React from "react";
import { cn } from "@/lib/utils";

const stickerRotations = [
  "hover:-rotate-1",
  "hover:rotate-0.5",
  "hover:-rotate-0.5",
  "hover:rotate-1"
];

interface StickerProps {
  children: React.ReactNode;
  className?: string;
}

export function Sticker({ children, className }: StickerProps) {
  const rotation = React.useMemo(() => 
    stickerRotations[Math.floor(Math.random() * stickerRotations.length)], 
  []);

  return (
    <span className={cn(
      "inline-flex items-center gap-1 rounded-none border-2 border-black bg-secondary px-2 py-0.5 text-[10px] font-black uppercase text-white shadow-neo-sm transition-transform duration-200 will-change-transform",
      rotation.replace("hover:", ""), 
      rotation,
      className
    )}>
      {children}
    </span>
  );
}
