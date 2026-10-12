import type { SshAuditEntry } from "./messages";

// P47 — o registro de uso do SSH: o que cada chamada das tools de SSH pediu e como terminou, em palavras de gente. O log guarda o
// comando (que pode ter um segredo) e nunca a saída.

const APPROVAL: Record<string, string> = {
  not_required: "sem aprovação",
  approved: "aprovada",
  denied: "recusada",
  unavailable: "ninguém para aprovar",
};

const ACTION: Record<string, string> = { exec: "comando", upload: "envio", download: "download" };

/** A aprovação como a pessoa lê; o que o hub mandar e esta tela não conhecer aparece como veio. */
export function approvalLabel(approval: string): string {
  return APPROVAL[approval] ?? approval;
}

export function actionLabel(action: string): string {
  return ACTION[action] ?? action;
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
  return `${from} → ${to}${entry.overwrite ? " (sobrescreve)" : ""}`;
}

function size(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} KB`;
  return `${(bytes / (1024 * 1024)).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB`;
}

/** Como terminou, em poucas palavras; vazio quando nada rodou e não há motivo a dar. */
export function outcomeOf(entry: SshAuditEntry): string {
  if (entry.error !== undefined) return `erro: ${entry.error}`;
  if (entry.timedOut) return "tempo esgotado";
  if (entry.exitCode !== undefined) return `saiu com ${entry.exitCode}`;
  if (entry.bytes !== undefined) return size(entry.bytes);
  return "";
}

/** `12/10 14:03:11`, na hora de quem olha. */
export function whenLabel(atMs: number): string {
  return new Date(atMs).toLocaleString("pt-BR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).replace(",", "");
}
