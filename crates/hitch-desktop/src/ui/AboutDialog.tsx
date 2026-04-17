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
      <DialogContent className="max-w-sm text-center">
        <div className="flex flex-col items-center gap-6 py-6">
          <HitchIcon size="lg" className="transform rotate-6" />
          <div className="space-y-2">
            <h2 className="text-2xl font-black uppercase tracking-tight">{info?.name ?? "HITCH DESKTOP"}</h2>
            <div className="inline-block border-2 border-black bg-secondary px-3 py-1 text-xs font-black text-white shadow-neo-sm transform -rotate-2">
              VERSION {info?.version ?? "0.0.0"}
            </div>
          </div>
          <div className="text-[10px] font-black uppercase tracking-widest text-black/40">
            © 2026 DOOMEDRAMEN
          </div>
        </div>
        <DialogFooter className="sm:justify-center">
          <Button variant="default" onClick={() => onOpenChange(false)} className="min-w-32">
            STAY NEUTRAL
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
