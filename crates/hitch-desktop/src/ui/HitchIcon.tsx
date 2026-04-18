import React from "react";
import { cn } from "@/lib/utils";

export function HitchIcon({ className, size = "md", variant = "default", noShadow = false }: { className?: string; size?: "sm" | "md" | "lg"; variant?: "default" | "inverted"; noShadow?: boolean }) {
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

  const variants = {
    default: "bg-primary text-black",
    inverted: "bg-white text-black",
  };

  return (
    <div className={cn(!noShadow && "shadow-neo-sm", "border-2 border-black flex items-center justify-center", variants[variant], sizes[size], className)}>
      <span className={cn("font-black italic select-none", fontSizes[size])}>H</span>
    </div>
  );
}

// SVG Version for easy export
export const HitchIconSvg = () => (
  <svg width="100" height="100" viewBox="0 0 100 100" fill="none" xmlns="http://www.w3.org/2000/svg">
    <rect x="2" y="2" width="96" height="96" fill="#FFC107" stroke="black" strokeWidth="4"/>
    <text x="50" y="55" font-family="sans-serif" font-weight="900" font-style="italic" font-size="60" fill="black" text-anchor="middle" dominant-baseline="middle">H</text>
  </svg>
);
