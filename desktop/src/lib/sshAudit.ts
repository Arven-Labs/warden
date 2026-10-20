import { locale, t, type Key } from "../i18n/index.ts";
import type { SshAuditEntry } from "../types";

// P47 — the words of the SSH usage log: what each call of the SSH tools asked for and how it ended. The log keeps the command (which
// can hold a secret) and never its output.

const APPROVALS = new Set(["not_required", "approved", "denied", "unavailable"]);
const ACTIONS = new Set(["exec", "upload", "download"]);

/** The tag for dates and numbers: day first in both languages, so English reads as British. */
const tag = () => (locale() === "pt-BR" ? "pt-BR" : "en-GB");

/** The approval as a person reads it; one the screen doesn't know shows as it came. */
export function approvalLabel(approval: string): string {
  return APPROVALS.has(approval) ? t(`ssh.approval.${approval}` as Key) : approval;
}

export function actionLabel(action: string): string {
  return ACTIONS.has(action) ? t(`ssh.action.${action}` as Key) : action;
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
  return `${from} → ${to}${entry.overwrite ? t("ssh.overwrite") : ""}`;
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toLocaleString(tag(), { maximumFractionDigits: 1 })} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString(tag(), { maximumFractionDigits: 1 })} MB`;
}

/** How it ended, in a few words; empty when nothing ran and there is no reason to give. */
export function outcomeOf(entry: SshAuditEntry): string {
  if (entry.error !== undefined) return t("ssh.error", { error: entry.error });
  if (entry.timedOut) return t("ssh.timedOut");
  if (entry.exitCode !== undefined) return t("ssh.exit", { code: entry.exitCode });
  if (entry.bytes !== undefined) return size(entry.bytes);
  return "";
}

/** `12/10 14:03:11`, in the viewer's time. */
export function whenLabel(atMs: number): string {
  return new Date(atMs).toLocaleString(tag(), { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).replace(",", "");
}
