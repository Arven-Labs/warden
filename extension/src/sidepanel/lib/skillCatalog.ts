// P140 — the pure half of the gallery of ready-made skills in the skills tab (`SkillsView.tsx`). Run its tests with `npm test`.

import type { CatalogSkillDto } from "../../protocol/messages";

/** What a gallery row shows, from the entry's state in the vault and whether the person already asked to replace theirs:
 * `installed` (nothing to do), `install` (one tap), `conflict` (a skill of that name that differs: say so and offer to use the
 * gallery's) or `confirm` (asked once — replacing takes a second, explicit tap). */
export type CatalogAction = "installed" | "install" | "conflict" | "confirm";

export function catalogAction(status: CatalogSkillDto["status"], confirming: boolean): CatalogAction {
  if (status === "installed") return "installed";
  if (status === "modified") return confirming ? "confirm" : "conflict";
  return "install";
}

// P136 — the descriptions of the gallery come from the hub in English, so only Portuguese needs a translation. A skill that is not
// here (a newer one) shows the hub's text as it is. Same text as `web/src/i18n/catalog.ts`.
const ptBR: Record<string, string> = {
  "code-review-checklist":
    "Revisar uma mudança de código quanto a bugs, riscos e clareza, com os achados ordenados por gravidade. Use quando a pessoa compartilhar um diff, uma função ou um pull request e pedir uma revisão.",
  "commit-message":
    "Escrever uma boa mensagem de commit a partir de um diff ou da descrição de uma mudança. Use quando a pessoa pedir uma mensagem de commit ou o título e a descrição de um pull request.",
  "decision-record":
    "Registrar uma decisão com o contexto, as opções consideradas e os motivos, para que dê para entendê-la depois. Use quando a pessoa fechar uma escolha técnica ou de produto e quiser registrá-la.",
  "email-draft":
    "Redigir um e-mail claro e educado a partir de alguns pontos. Use quando a pessoa pedir para escrever, responder ou reescrever um e-mail ou uma mensagem para alguém.",
  "explain-simply":
    "Explicar uma ideia difícil em palavras simples, com um exemplo do dia a dia. Use quando a pessoa disser que não entendeu algo, pedir uma explicação simples ou pedir para explicar como se fosse nova no assunto.",
  "meeting-summary":
    "Transformar anotações ou a transcrição de uma reunião num resumo curto com decisões, ações e perguntas em aberto. Use quando a pessoa colar anotações de uma reunião ou perguntar o que ficou combinado.",
  "research-brief":
    "Produzir um resumo de pesquisa curto e honesto sobre um tema, separando o que se sabe do que é incerto. Use quando pedirem para pesquisar, comparar opções ou dar uma visão geral de um assunto.",
  "weekly-summary":
    "Montar a atualização semanal de status a partir das anotações, tarefas ou mensagens da pessoa. Use quando pedirem um relatório semanal, uma atualização de status ou um resumo da semana.",
};

/** The Portuguese descriptions, by skill name: what the test checks against the hub's catalog files. */
export const PT_BR_DESCRIPTIONS: Readonly<Record<string, string>> = ptBR;

/** The gallery's description of the skill `name` in `locale`; `fallback` (the hub's text) when there is no translation. */
export function catalogDescription(locale: string, name: string, fallback: string): string {
  return (locale === "pt-BR" ? ptBR[name] : undefined) ?? fallback;
}
