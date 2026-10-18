// P157 — como a lista guarda as conversas soltas (fora de projeto): as fixadas primeiro, depois por quão recentes são, e as arquivadas à
// parte. Pura, para testar sem janela. Espelha `web/src/hub/conversationGroups.ts`.

import type { ConversationSummary } from "./messages";

type Groupable = Pick<ConversationSummary, "updatedAt" | "pinned" | "archived">;

/** Um bloco da lista: o nome e as conversas dele, na ordem em que vieram. */
export interface ConversationBucket<T> {
  label: string;
  items: T[];
}

export interface GroupedConversations<T> {
  pinned: T[];
  buckets: ConversationBucket<T>[];
  archived: T[];
}

const DAY_MS = 24 * 60 * 60 * 1000;

/** Os blocos por data, do mais recente ao mais antigo. */
const LABELS = ["Hoje", "Ontem", "Últimos 7 dias", "Últimos 30 dias", "Anteriores"];

/** O começo do dia de `now` no relógio deste computador. */
function startOfDay(now: number): number {
  const date = new Date(now);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** Em que bloco cai uma conversa usada pela última vez em `updatedAt`. O dia conta no calendário, não em 24 horas para trás: "Ontem" é o dia
 * inteiro de ontem. */
export function bucketLabel(updatedAt: number, now: number): string {
  const today = startOfDay(now);
  if (updatedAt >= today) return LABELS[0];
  if (updatedAt >= startOfDay(today - DAY_MS / 2)) return LABELS[1];
  if (updatedAt >= startOfDay(today - 6.5 * DAY_MS)) return LABELS[2];
  if (updatedAt >= startOfDay(today - 29.5 * DAY_MS)) return LABELS[3];
  return LABELS[4];
}

/** Separa `conversations` em fixadas, blocos por data e arquivadas. Dentro de cada parte a ordem de chegada se mantém, e um bloco sem nada
 * fica de fora. Uma conversa arquivada só aparece em `archived`, diga o que disser o resto. */
export function groupConversations<T extends Groupable>(conversations: T[], now: number): GroupedConversations<T> {
  const pinned: T[] = [];
  const archived: T[] = [];
  const byLabel = new Map<string, T[]>();
  for (const conversation of conversations) {
    if (conversation.archived) archived.push(conversation);
    else if (conversation.pinned) pinned.push(conversation);
    else {
      const label = bucketLabel(conversation.updatedAt, now);
      byLabel.set(label, [...(byLabel.get(label) ?? []), conversation]);
    }
  }
  const buckets = LABELS.filter((label) => byLabel.has(label)).map((label) => ({ label, items: byLabel.get(label)! }));
  return { pinned, buckets, archived };
}
