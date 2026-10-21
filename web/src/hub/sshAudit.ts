import { intlTag, t, type Key } from "../i18n/index.ts";
import type { SshAuditEntry } from "./messages";

// P47 — the SSH usage log: what each call to the SSH tools asked for and how it ended, in plain words. The log keeps the
// command (which may hold a secret) and never the output.

const APPROVAL: Record<string, Key> = {
  not_required: "ssh.approval.not_required",
  approved: "ssh.approval.approved",
  denied: "ssh.approval.denied",
  unavailable: "ssh.approval.unavailable",
};

const ACTION: Record<string, Key> = { exec: "ssh.action.exec", upload: "ssh.action.upload", download: "ssh.action.download" };

/** The approval as the person reads it; whatever the hub sends that this screen does not know shows as it came. */
export function approvalLabel(approval: string): string {
  return APPROVAL[approval] ? t(APPROVAL[approval]) : approval;
}

export function actionLabel(action: string): string {
  return ACTION[action] ? t(ACTION[action]) : action;
}

/** Whether the call was blocked before it ran: the person refused, or there was nobody to ask. */
export function wasRefused(entry: SshAuditEntry): boolean {
  return entry.approval === "denied" || entry.approval === "unavailable";
}

/** What it was about: the command, or `source → destination` for a copy (a download comes from the server). */
export function detailOf(entry: SshAuditEntry): string {
  if (entry.command !== undefined) return entry.command;
  const local = entry.localPath ?? "?";
  const remote = entry.remotePath ?? "?";
  const [from, to] = entry.action === "download" ? [remote, local] : [local, remote];
  return `${from} → ${to}${entry.overwrite ? ` ${t("ssh.overwrites")}` : ""}`;
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toLocaleString(intlTag(), { maximumFractionDigits: 1 })} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString(intlTag(), { maximumFractionDigits: 1 })} MB`;
}

/** How it ended, in a few words; empty when nothing ran and there is no reason to give. */
export function outcomeOf(entry: SshAuditEntry): string {
  if (entry.error !== undefined) return t("ssh.error", { error: entry.error });
  if (entry.timedOut) return t("ssh.timedOut");
  if (entry.exitCode !== undefined) return t("ssh.exited", { code: entry.exitCode });
  if (entry.bytes !== undefined) return size(entry.bytes);
  return "";
}

/** `12/10 14:03:11`, na hora de quem olha. */
export function whenLabel(atMs: number): string {
  return new Date(atMs).toLocaleString(intlTag(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).replace(",", "");
}
