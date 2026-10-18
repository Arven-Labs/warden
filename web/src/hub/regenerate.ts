// P157 — a parte pura de "regenerar a resposta" e "editar uma mensagem minha": de qual mensagem o refazer parte e como fica a lista cortada.
// Pura, para testar sem janela. Espelha `desktop/src/lib/regenerate.ts`, mas sobre as entradas da conversa. Refazer manda o turno de novo
// com o id da mensagem de onde parte (`replaceFrom`); o turno toma o lugar dela e de tudo o que veio depois. O id só existe numa entrada que
// o hub já guardou (`withMessageIds`), então uma entrada sem id não se refaz.

import type { ChatEntry } from "./connection";

/** A última mensagem que a pessoa mandou e que o hub já guardou, ou `undefined`. */
export function lastUserEntry(entries: ChatEntry[]): ChatEntry | undefined {
  for (let i = entries.length - 1; i >= 0; i--) {
    if (entries[i].role === "user" && entries[i].id) return entries[i];
  }
  return undefined;
}

/** Se `entry` é a última resposta do assistente (as de erro não contam) e dá para regenerá-la. */
export function isLastAnswer(entries: ChatEntry[], index: number): boolean {
  const entry = entries[index];
  if (!entry || entry.role !== "assistant" || !entry.id) return false;
  return !entries.slice(index + 1).some((e) => e.role !== "error");
}

/** As entradas antes da de id `from`; todas, se ela não está na lista. */
export function entriesBefore(entries: ChatEntry[], from: string): ChatEntry[] {
  const at = entries.findIndex((e) => e.id === from);
  return at === -1 ? entries : entries.slice(0, at);
}

/** Quantas mensagens vêm depois da de id `from` (0 se é a última ou não está): o que uma edição também apaga. Os erros não contam, o hub não os guarda. */
export function messagesAfter(entries: ChatEntry[], from: string): number {
  const at = entries.findIndex((e) => e.id === from);
  return at === -1 ? 0 : entries.slice(at + 1).filter((e) => e.role !== "error").length;
}

/** Por que uma conversa não pode ser refeita agora, ou `null` quando pode: ainda não está no hub, é uma thread (o contexto dela é de outra
 * conversa), é de um projeto de código (a sessão do motor não volta atrás), há um turno em andamento ou o hub está fora. */
export function redoBlock(state: { onHub: boolean; thread: boolean; code: boolean; busy: boolean; offline: boolean }): string | null {
  if (state.offline) return "sem conexão com o hub";
  if (state.busy) return "há uma resposta a caminho";
  if (!state.onHub) return "a conversa ainda não está no hub";
  if (state.thread) return "uma thread não se refaz";
  if (state.code) return "uma conversa de projeto de código não se refaz";
  return null;
}
