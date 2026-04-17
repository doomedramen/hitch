import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-none text-sm font-black uppercase tracking-tight transition-all focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50 border-2 border-black",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground shadow-neo-sm hover:translate-x-[-2px] hover:translate-y-[-2px] hover:shadow-neo hover:bg-primary active:translate-x-[0px] active:translate-y-[0px] active:shadow-none",
        outline:
          "bg-white text-black shadow-neo-sm hover:translate-x-[-2px] hover:translate-y-[-2px] hover:shadow-neo hover:bg-accent-hover/20 active:translate-x-[0px] active:translate-y-[0px] active:shadow-none",
        secondary: "bg-secondary text-secondary-foreground shadow-neo-sm hover:translate-x-[-2px] hover:translate-y-[-2px] hover:shadow-neo active:translate-x-[0px] active:translate-y-[0px] active:shadow-none",
        ghost: "border-transparent hover:bg-black/5 hover:border-black/10",
        destructive:
          "bg-destructive text-destructive-foreground shadow-neo-sm hover:translate-x-[-2px] hover:translate-y-[-2px] hover:shadow-neo active:translate-x-[0px] active:translate-y-[0px] active:shadow-none"
      },
      size: {
        default: "h-11 px-6",
        sm: "h-9 px-4 text-xs",
        lg: "h-14 px-10 text-base",
        icon: "h-11 w-11"
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
