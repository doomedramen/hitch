import React, { useEffect, useState } from "react";
import { getName, getVersion } from "@tauri-apps/api/app";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
} from "@/components/ui/dialog";
import { HitchIcon } from "./index";

export function AboutDialog({ open, onOpenChange }: { open: boolean, onOpenChange: (v: boolean) => void }) {
  const [info, setInfo] = useState<{ name: string, version: string } | null>(null);

  useEffect(() => {
    if (open && !info) {
      void (async () => {
        try {
          const [n, v] = await Promise.all([getName(), getVersion()]);
          setInfo({ name: n, version: v });
        } catch {
          setInfo({ name: "HITCH DESKTOP", version: "0.0.0" });
        }
      })();
    }
  }, [open, info]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-xs text-center">
        <div className="flex flex-col items-center gap-5 py-5">
          <HitchIcon size="lg" />
          <div className="space-y-2">
            <h2 className="text-[17px] font-semibold tracking-tight text-label">{info?.name ?? "Hitch Desktop"}</h2>
            <div className="inline-flex rounded-full bg-[var(--fill-soft)] px-2.5 py-0.5 text-[12px] font-medium text-label-secondary">
              Version {info?.version ?? "0.0.0"}
            </div>
          </div>
          <div className="text-[11px] text-label-tertiary">
            © 2026 doomedramen
          </div>
        </div>
        <DialogFooter className="sm:justify-center">
          <Button variant="default" onClick={() => onOpenChange(false)} className="min-w-24">
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
