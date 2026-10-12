import type { SshAuditEntry } from "../types";

// P47 — the words of the SSH usage log: what each call of the SSH tools asked for and how it ended. The log keeps the command (which
// can hold a secret) and never its output.

const APPROVAL: Record<string, string> = {
  not_required: "no approval needed",
  approved: "approved",
  denied: "denied",
  unavailable: "nobody to ask",
};

const ACTION: Record<string, string> = { exec: "command", upload: "upload", download: "download" };

/** The approval as a person reads it; one the screen doesn't know shows as it came. */
export function approvalLabel(approval: string): string {
  return APPROVAL[approval] ?? approval;
}

export function actionLabel(action: string): string {
  return ACTION[action] ?? action;
}

/** Whether the call was stopped before it ran: a person said no, or there was nobody to ask. */
export function wasRefused(entry: SshAuditEntry): boolean {
  return entry.approval === "denied" || entry.approval === "unavailable";
}

/** What it was about: the command, or `from → to` for a copy (a download leaves the server). */
export function detailOf(entry: SshAuditEntry): string {
  if (entry.command !== undefined) return entry.command;
  const local = entry.localPath ?? "?";
  const remote = entry.remotePath ?? "?";
  const [from, to] = entry.action === "download" ? [remote, local] : [local, remote];
  return `${from} → ${to}${entry.overwrite ? " (overwrite)" : ""}`;
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toLocaleString("en-GB", { maximumFractionDigits: 1 })} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString("en-GB", { maximumFractionDigits: 1 })} MB`;
}

/** How it ended, in a few words; empty when nothing ran and there is no reason to give. */
export function outcomeOf(entry: SshAuditEntry): string {
  if (entry.error !== undefined) return `error: ${entry.error}`;
  if (entry.timedOut) return "timed out";
  if (entry.exitCode !== undefined) return `exit ${entry.exitCode}`;
  if (entry.bytes !== undefined) return size(entry.bytes);
  return "";
}

/** `12/10 14:03:11`, in the viewer's time. */
export function whenLabel(atMs: number): string {
  return new Date(atMs).toLocaleString("en-GB", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).replace(",", "");
}
