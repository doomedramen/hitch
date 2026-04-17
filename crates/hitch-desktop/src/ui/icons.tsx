import React from "react";
import {
  CheckCircle2,
  CircleX,
  Cloud,
  GitCommit,
  HardDrive,
  Info,
  TriangleAlert,
  Wrench
} from "lucide-react";
import type { OutputLevel, RepoIdentity, TimelineKind } from "./types";

export function RepoIdentityIcon({
  identity,
  className,
  strokeWidth = 3
}: {
  identity: RepoIdentity;
  className?: string;
  strokeWidth?: number;
}) {
  const Icon = identity.kind === "origin" ? Cloud : HardDrive;
  return <Icon className={className} strokeWidth={strokeWidth} aria-hidden="true" />;
}

export function TimelineKindIcon({
  kind,
  className,
  strokeWidth = 3
}: {
  kind: TimelineKind;
  className?: string;
  strokeWidth?: number;
}) {
  const Icon = kind === "GitCommit" ? GitCommit : Wrench;
  return <Icon className={className} strokeWidth={strokeWidth} aria-hidden="true" />;
}

export function OutputLevelIcon({
  level,
  className,
  strokeWidth = 3
}: {
  level: OutputLevel;
  className?: string;
  strokeWidth?: number;
}) {
  const Icon =
    level === "Info"
      ? Info
      : level === "Success"
        ? CheckCircle2
        : level === "Warning"
          ? TriangleAlert
          : CircleX;
  return <Icon className={className} strokeWidth={strokeWidth} aria-hidden="true" />;
}

