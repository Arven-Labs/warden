// P136 — the descriptions of the ready-made skills (the gallery). They come from the hub in English, from the files in
// `crates/warden-core/catalog/skills/`, so they are not in `en.ts`: English is the hub's own text, and only Portuguese needs a
// translation. A skill that is not here (a newer one) shows the hub's text as it is.

import { locale } from "./index.ts";

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

/** The gallery's description of the skill `name` in the current language; `fallback` (the hub's text) when there is no translation. */
export function catalogDescription(name: string, fallback: string): string {
  return (locale() === "pt-BR" ? ptBR[name] : undefined) ?? fallback;
}
