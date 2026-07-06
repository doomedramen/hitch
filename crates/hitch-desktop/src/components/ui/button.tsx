import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-[6px] text-[13px] font-medium leading-none transition-colors select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60 disabled:pointer-events-none disabled:opacity-40",
  {
    variants: {
      variant: {
        default:
          "bg-primary text-primary-foreground shadow-[0_1px_1px_rgba(0,0,0,0.1)] hover:bg-primary/90 active:bg-primary/80",
        outline:
          "bg-[var(--control-bg)] text-label shadow-control hover:bg-[var(--control-hover)]",
        secondary:
          "bg-[var(--control-bg)] text-label shadow-control hover:bg-[var(--control-hover)]",
        ghost: "text-label hover:bg-[var(--fill-soft)]",
        destructive:
          "bg-destructive text-destructive-foreground shadow-[0_1px_1px_rgba(0,0,0,0.1)] hover:bg-destructive/90 active:bg-destructive/80"
      },
      size: {
        default: "h-7 px-3",
        sm: "h-[26px] px-2.5 text-[12px]",
        lg: "h-9 px-4",
        icon: "h-7 w-7"
      }
    },
    defaultVariants: {
      variant: "default",
      size: "default"
    }
  }
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  }
);
Button.displayName = "Button";

export { Button, buttonVariants };
