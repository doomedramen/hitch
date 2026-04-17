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
          "flex h-10 w-full rounded-none border-2 border-black bg-white px-3 py-1 text-sm shadow-neo-sm transition-all file:border-0 file:bg-transparent file:text-sm file:font-bold placeholder:text-muted-foreground focus-visible:outline-none focus-visible:shadow-[inset_2px_2px_0px_0px_rgba(0,0,0,0.2)] disabled:cursor-not-allowed disabled:opacity-50 ring-offset-background",
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

