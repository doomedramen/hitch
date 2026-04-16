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
  className
}: {
  identity: RepoIdentity;
  className?: string;
}) {
  const Icon = identity.kind === "origin" ? Cloud : HardDrive;
  return <Icon className={className} aria-hidden="true" />;
}

export function TimelineKindIcon({
  kind,
  className
}: {
  kind: TimelineKind;
  className?: string;
}) {
  const Icon = kind === "GitCommit" ? GitCommit : Wrench;
  return <Icon className={className} aria-hidden="true" />;
}

export function OutputLevelIcon({
  level,
  className
}: {
  level: OutputLevel;
  className?: string;
}) {
  const Icon =
    level === "Info"
      ? Info
      : level === "Success"
        ? CheckCircle2
        : level === "Warning"
          ? TriangleAlert
          : CircleX;
  return <Icon className={className} aria-hidden="true" />;
}

