import { intlTag, t, type Key } from "../i18n/index.ts";
import type { SshAuditEntry } from "./messages";

// P47 — o registro de uso do SSH: o que cada chamada das tools de SSH pediu e como terminou, em palavras de gente. O log guarda o
// comando (que pode ter um segredo) e nunca a saída.

const APPROVAL: Record<string, Key> = {
  not_required: "ssh.approval.not_required",
  approved: "ssh.approval.approved",
  denied: "ssh.approval.denied",
  unavailable: "ssh.approval.unavailable",
};

const ACTION: Record<string, Key> = { exec: "ssh.action.exec", upload: "ssh.action.upload", download: "ssh.action.download" };

/** A aprovação como a pessoa lê; o que o hub mandar e esta tela não conhecer aparece como veio. */
export function approvalLabel(approval: string): string {
  return APPROVAL[approval] ? t(APPROVAL[approval]) : approval;
}

export function actionLabel(action: string): string {
  return ACTION[action] ? t(ACTION[action]) : action;
}

/** Se a chamada foi barrada antes de rodar: a pessoa recusou, ou não havia ninguém para perguntar. */
export function wasRefused(entry: SshAuditEntry): boolean {
  return entry.approval === "denied" || entry.approval === "unavailable";
}

/** Do que se tratava: o comando, ou `origem → destino` numa cópia (um download sai do servidor). */
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

/** Como terminou, em poucas palavras; vazio quando nada rodou e não há motivo a dar. */
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
