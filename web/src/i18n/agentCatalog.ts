// P136 — the role and the description of the ready-made agents (the agents gallery). They come from the hub in English, from the files in
// `crates/warden-bootstrap/catalog/agents/`, so they are not in `en.ts`: English is the hub's own text, and only Portuguese needs a
// translation. An agent that is not here (a newer one) shows the hub's text as it is. The persona is the model's prompt and is not shown.

import { locale } from "./index.ts";

interface Text {
  role: string;
  description: string;
}

const ptBR: Record<string, Text> = {
  "code-reviewer": {
    role: "Revisor de código",
    description: "Revisa uma mudança quanto a bugs, riscos e clareza e ajuda a escrever o commit. Lê, nunca edita.",
  },
  researcher: {
    role: "Pesquisador",
    description: "Reúne o que se sabe sobre um tema e diz o que ainda é incerto. Lê os seus arquivos, não muda nada.",
  },
  secretary: {
    role: "Secretário",
    description: "Transforma anotações de reunião e o trabalho da semana em resumos, decisões e próximos passos. Lê as suas anotações, não muda nada.",
  },
  writer: {
    role: "Redator",
    description: "Redige e melhora e-mails, mensagens e textos curtos no seu tom. Pergunta quando o objetivo ou o leitor não está claro.",
  },
};

/** The Portuguese texts, by agent id: what the test checks against the hub's catalog files. */
export const PT_BR_AGENTS: Readonly<Record<string, Text>> = ptBR;

/** The gallery's role and description of the agent `id` in the current language; the hub's own text when there is no translation. */
export function catalogAgentText(id: string, fallback: Text): Text {
  return (locale() === "pt-BR" ? ptBR[id] : undefined) ?? fallback;
}
