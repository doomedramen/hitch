import React from "react";
import { cn } from "@/lib/utils";

export function HitchIcon({ className, size = "md" }: { className?: string; size?: "sm" | "md" | "lg" }) {
  const sizes = {
    sm: "h-5 w-5",
    md: "h-6 w-6",
    lg: "h-20 w-20",
  };
  const fontSizes = {
    sm: "text-[8px]",
    md: "text-[10px]",
    lg: "text-4xl",
  };

  return (
    <div className={cn("border-2 border-black bg-primary flex items-center justify-center shadow-neo-sm", sizes[size], className)}>
      <span className={cn("font-black italic text-black", fontSizes[size])}>H</span>
    </div>
  );
}
