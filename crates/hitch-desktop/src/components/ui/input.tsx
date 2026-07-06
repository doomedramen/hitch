import * as React from "react";

import { cn } from "@/lib/utils";

export interface InputProps
  extends React.InputHTMLAttributes<HTMLInputElement> {}

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-7 w-full rounded-[6px] border-[0.5px] border-separator-strong bg-[var(--control-bg)] px-2.5 text-[13px] text-label shadow-control transition-shadow file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-label-tertiary focus-visible:outline-none focus-visible:border-transparent focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-40",
          className
        )}
        ref={ref}
        {...props}
      />
    );
  }
);
Input.displayName = "Input";

export { Input };

