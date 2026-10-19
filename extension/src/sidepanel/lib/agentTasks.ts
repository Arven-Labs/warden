// P123 — o trabalho que os agentes passam uns aos outros, como a aba "Trabalho dos agentes" mostra: as tarefas que um turno
// começou formam um grupo, com um progresso contado sobre elas. Espelha `desktop/src/lib/agentTasks.ts`.

import { intlTag, t } from "../../i18n/index.ts";
import type { AgentTask, AgentTaskAction, AgentTaskState } from "../../protocol/messages";

/** Uma tarefa como a lista a mostra: quantos níveis abaixo do agente do turno ela está (0 para as que o próprio agente começou). */
export interface TaskRow {
  task: AgentTask;
  depth: number;
}

export interface TaskGroup {
  group: string;
  /** O agente que delegou, se houve. */
  owner: string | null;
  /** Da mais antiga à mais nova, na ordem em que o turno as começou. */
  tasks: AgentTask[];
  /** As mesmas tarefas como árvore achatada para mostrar: cada uma seguida das suas subtarefas. */
  rows: TaskRow[];
  total: number;
  /** Concluídas, falhas ou canceladas: nada mais a esperar. */
  finished: number;
  counts: Record<AgentTaskState, number>;
  /** Percentual inteiro das tarefas que terminaram. */
  percent: number;
  /** Tokens usados pelas tarefas que informaram. */
  totalTokens: number;
  /** Algo no grupo ainda está pendente ou rodando. */
  active: boolean;
  createdAtMs: number;
}

// Getters, so each read says the words of the language the person has now.
export const STATE_LABEL: Record<AgentTaskState, string> = {
  get pending() {
    return t("taskState.pending");
  },
  get running() {
    return t("taskState.running");
  },
  get waiting() {
    return t("taskState.waiting");
  },
  get paused() {
    return t("taskState.paused");
  },
  get done() {
    return t("taskState.done");
  },
  get failed() {
    return t("taskState.failed");
  },
  get cancelled() {
    return t("taskState.cancelled");
  },
};

export const STATE_MARK: Record<AgentTaskState, string> = {
  pending: "○",
  running: "◐",
  waiting: "◉",
  paused: "⏸",
  done: "✓",
  failed: "⚠",
  cancelled: "⏹",
};

const emptyCounts = (): Record<AgentTaskState, number> => ({ pending: 0, running: 0, waiting: 0, paused: 0, done: 0, failed: 0, cancelled: 0 });

/** O que dá para fazer com esta tarefa agora: só a que roda no processo do hub que respondeu. A que ainda não começou só pode ser parada;
 * a pausada pode ser retomada; uma delegação que o agente espera também só pode ser parada. */
export function actionsFor(task: AgentTask): AgentTaskAction[] {
  if (!task.controllable) return [];
  switch (task.state) {
    case "pending":
      return ["cancel"];
    case "running":
    case "waiting":
      return task.pausable ? ["pause", "cancel"] : ["cancel"];
    case "paused":
      return ["resume", "cancel"];
    default:
      return [];
  }
}

export const ACTION_LABEL: Record<AgentTaskAction, string> = {
  get pause() {
    return t("taskAction.pause");
  },
  get resume() {
    return t("taskAction.resume");
  },
  get cancel() {
    return t("taskAction.cancel");
  },
};

/** As tarefas de um grupo como árvore: cada tarefa seguida das suas subtarefas, um nível abaixo. Uma tarefa cujo pai não está no grupo
 * (ou um laço que não deveria existir) aparece no topo em vez de se perder. */
function treeRows(ordered: AgentTask[]): TaskRow[] {
  const ids = new Set(ordered.map((t) => t.id));
  const rows: TaskRow[] = [];
  const seen = new Set<string>();
  const add = (task: AgentTask, depth: number) => {
    seen.add(task.id);
    rows.push({ task, depth });
    for (const child of ordered) if (child.parentId === task.id && !seen.has(child.id)) add(child, depth + 1);
  };
  for (const task of ordered) if (!task.parentId || !ids.has(task.parentId)) add(task, 0);
  for (const task of ordered) if (!seen.has(task.id)) add(task, 0);
  return rows;
}

/** As tarefas agrupadas pelo turno que as começou, o grupo mais novo primeiro. Um estado desconhecido conta como pendente. */
export function groupTasks(tasks: AgentTask[]): TaskGroup[] {
  const byGroup = new Map<string, AgentTask[]>();
  for (const task of tasks) {
    const list = byGroup.get(task.group);
    if (list) list.push(task);
    else byGroup.set(task.group, [task]);
  }
  const groups: TaskGroup[] = [];
  for (const [group, members] of byGroup) {
    const ordered = [...members].sort((a, b) => a.createdAtMs - b.createdAtMs);
    const counts = emptyCounts();
    for (const task of ordered) counts[task.state in counts ? task.state : "pending"] += 1;
    const finished = counts.done + counts.failed + counts.cancelled;
    groups.push({
      group,
      owner: ordered.find((t) => t.owner)?.owner ?? null,
      tasks: ordered,
      rows: treeRows(ordered),
      total: ordered.length,
      finished,
      counts,
      percent: Math.round((finished / ordered.length) * 100),
      totalTokens: ordered.reduce((sum, t) => sum + (t.totalTokens ?? 0), 0),
      active: counts.pending + counts.running + counts.waiting + counts.paused > 0,
      createdAtMs: ordered[0].createdAtMs,
    });
  }
  return groups.sort((a, b) => b.createdAtMs - a.createdAtMs);
}

/** As tarefas que envolvem um agente: as que ele recebeu e as que ele delegou (P120, a partir de um nó da árvore). */
export function involvingAgent(tasks: AgentTask[], agent: string): AgentTask[] {
  return tasks.filter((t) => t.assignee === agent || t.owner === agent);
}

/** O que um agente andou fazendo, tirado só das tarefas que o hub guarda (P120): o que ele recebeu (`assignee`), quantas delegou (`owner`) e
 * quando mexeu em algo pela última vez. O log guarda só as mais recentes, e conversas diretas e recados não entram. */
export interface AgentActivity {
  /** Tarefas que ele recebeu, por onde estão. */
  done: number;
  failed: number;
  cancelled: number;
  /** Pendentes, rodando, aguardando agente ou pausadas. */
  active: number;
  /** Tokens das tarefas que ele recebeu e que informaram. */
  tokens: number;
  /** Quantas tarefas ele delegou a outros. */
  delegated: number;
  /** A última coisa que aconteceu: numa tarefa dele (criada, começada ou terminada) ou numa conversa com ele. `null` quando só se sabe o gasto. */
  lastActiveMs: number | null;
  /** O que ele gastou na janela do livro-caixa do hub. Só aparece quando a tela o informa. */
  spend?: AgentSpend;
}

/** O gasto de um agente no livro-caixa do hub, que só guarda as últimas `windowHours` (o maior limite). */
export interface AgentSpend {
  costUsd: number;
  /** Chamadas sem preço conhecido: o valor em dólar as deixa de fora. */
  unpricedCalls: number;
  windowHours: number;
}

/** O que a atividade usa além das tarefas (P123): o gasto e a conversa mais recente do agente. */
export interface ActivityExtras {
  spend?: AgentSpend;
  lastConversationMs?: number;
}

/** A atividade de `agent`, ou `null` quando nada o envolve: nem tarefa, nem conversa recente, nem gasto. */
export function activityOf(tasks: AgentTask[], agent: string, extras: ActivityExtras = {}): AgentActivity | null {
  const received = tasks.filter((t) => t.assignee === agent);
  const delegated = tasks.filter((t) => t.owner === agent);
  const spend = extras.spend && (extras.spend.costUsd > 0 || extras.spend.unpricedCalls > 0) ? extras.spend : undefined;
  if (received.length === 0 && delegated.length === 0 && extras.lastConversationMs === undefined && !spend) return null;
  const counts = emptyCounts();
  for (const task of received) counts[task.state in counts ? task.state : "pending"] += 1;
  const stamp = (t: AgentTask) => t.finishedAtMs ?? t.startedAtMs ?? t.createdAtMs;
  const stamps = [...received, ...delegated].map(stamp);
  if (extras.lastConversationMs !== undefined) stamps.push(extras.lastConversationMs);
  return {
    done: counts.done,
    failed: counts.failed,
    cancelled: counts.cancelled,
    active: counts.pending + counts.running + counts.waiting + counts.paused,
    tokens: received.reduce((sum, t) => sum + (t.totalTokens ?? 0), 0),
    delegated: delegated.length,
    lastActiveMs: stamps.length > 0 ? Math.max(...stamps) : null,
    ...(spend && { spend }),
  };
}

/** O gasto de `agent` no relatório de uso do hub, ou `undefined` quando o hub não o conta (hub antigo, ou agente sem gasto na janela). */
export function spendOf(
  report: { recent?: { windowHours: number; byAgent?: { key: string; costUsd: number; unpricedCalls: number }[] } | null } | null,
  agent: string,
): AgentSpend | undefined {
  const bucket = report?.recent?.byAgent?.find((b) => b.key === agent);
  if (!bucket || !report?.recent) return undefined;
  return { costUsd: bucket.costUsd, unpricedCalls: bucket.unpricedCalls, windowHours: report.recent.windowHours };
}

/** Quando foi a última conversa de `agent` (a mais recente que falou com ele, sem contar as threads), ou `undefined` quando não houve. */
export function lastConversationOf(conversations: { agentId?: string | null; updatedAt: number; parent?: unknown }[], agent: string): number | undefined {
  const times = conversations.filter((c) => c.agentId === agent && !c.parent).map((c) => c.updatedAt);
  return times.length > 0 ? Math.max(...times) : undefined;
}

/** How long ago, in a few words: `now`, `4 min ago`, `2 h ago`, `3 d ago`. */
export function agoLabel(thenMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.round((nowMs - thenMs) / 60000));
  if (minutes < 1) return t("ago.now");
  if (minutes < 60) return t("ago.minutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return t("ago.hours", { count: hours });
  return t("ago.days", { count: Math.floor(hours / 24) });
}

/** The spend in a few words: `$1.20 in the last 24 h`, with a note of the calls the value leaves out. */
export function spendLabel(spend: AgentSpend): string {
  const window = t("spend.window", { hours: spend.windowHours });
  const unpriced = spend.unpricedCalls > 0 ? t(spend.unpricedCalls > 1 ? "spend.unpricedMany" : "spend.unpricedOne", { count: spend.unpricedCalls }) : "";
  if (spend.costUsd <= 0) return `${unpriced} ${window}`;
  const dollars = (n: number) => new Intl.NumberFormat(intlTag(), { style: "currency", currency: "USD" }).format(n);
  const money = spend.costUsd < 0.01 ? t("spend.lessThan", { amount: dollars(0.01) }) : dollars(spend.costUsd);
  return unpriced ? t("spend.withUnpriced", { money, window, unpriced }) : `${money} ${window}`;
}

export interface ComboSpendRow {
  combo: string;
  /** `US$ 1,20 nas últimas 24 h`, com as chamadas que o valor deixa de fora. */
  label: string;
}

/** P126 — o que cada combo gastou na janela do ledger, o maior primeiro. A janela é a do ledger, e não a dos tokens por combo (essa não tem limite
 * de tempo), por isso são linhas à parte. Um hub antigo, ou nenhuma chamada por um combo, não dá linha nenhuma. */
export function comboSpendRows(report: { recent?: { windowHours: number; byCombo?: { key: string; costUsd: number; unpricedCalls: number }[] } | null }): ComboSpendRow[] {
  const recent = report.recent;
  if (!recent?.byCombo) return [];
  return [...recent.byCombo]
    .sort((a, b) => b.costUsd - a.costUsd)
    .map((b) => ({ combo: b.key, label: spendLabel({ costUsd: b.costUsd, unpricedCalls: b.unpricedCalls, windowHours: recent.windowHours }) }));
}

/** A atividade numa linha do cartão da árvore: `3 concluídas, 1 falhou, 2 em andamento · 12,9 mil tokens · US$ 1,20 nas últimas 24 h · delegou 5 · ativo há 4 min`. */
export function activityLine(a: AgentActivity, nowMs: number): string {
  const parts = [
    a.done > 0 && t(a.done > 1 ? "tally.doneMany" : "tally.doneOne", { count: a.done }),
    a.failed > 0 && t("tally.failed", { count: a.failed }),
    a.cancelled > 0 && t(a.cancelled > 1 ? "tally.cancelledMany" : "tally.cancelledOne", { count: a.cancelled }),
    a.active > 0 && t("tally.active", { count: a.active }),
  ].filter(Boolean);
  const line = [parts.join(", ")];
  if (a.tokens > 0) line.push(t("tally.tokens", { count: formatTokens(a.tokens) }));
  if (a.spend) line.push(spendLabel(a.spend));
  if (a.delegated > 0) line.push(t("tally.delegated", { count: a.delegated }));
  if (a.lastActiveMs !== null) line.push(t("tally.activeAgo", { ago: agoLabel(a.lastActiveMs, nowMs) }));
  return line.filter(Boolean).join(" · ");
}

export function formatTokens(n: number): string {
  return new Intl.NumberFormat(intlTag(), { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** Quanto tempo uma tarefa levou (ou está levando): `4s`, `2m 05s`, `1h 03m`. `null` antes de começar. */
export function durationLabel(task: AgentTask, now: number): string | null {
  if (task.startedAtMs == null) return null;
  const seconds = Math.max(0, Math.round(((task.finishedAtMs ?? now) - task.startedAtMs) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}
