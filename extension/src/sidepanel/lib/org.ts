import type { OrgEdit } from "../../protocol/messages";

/** P120 — a organização dos agentes: cada um pode ter um cargo e um superior (`reportsTo`, o id de outro agente).
 * Por enquanto só aparece: não muda nada do que um agente pode fazer. Espelha `warden_bootstrap::org`. */
export interface OrgAgent {
  id: string;
  role?: string | null;
  reportsTo?: string | null;
}

export interface OrgNode<T extends OrgAgent = OrgAgent> {
  agent: T;
  children: OrgNode<T>[];
}

/** Os agentes como uma floresta: quem não tem superior fica no topo, na ordem em que vieram, com os seus subordinados
 * embaixo. Quem tem o superior sumido aparece no topo em vez de se perder, e um ciclo (que o hub recusa) nunca trava a árvore. */
export function buildOrg<T extends OrgAgent>(agents: T[]): OrgNode<T>[] {
  const ids = new Set(agents.map((a) => a.id));
  const seen = new Set<string>();
  const nodeOf = (agent: T): OrgNode<T> => {
    seen.add(agent.id);
    const children: OrgNode<T>[] = [];
    for (const report of agents) {
      if (report.reportsTo === agent.id && !seen.has(report.id)) children.push(nodeOf(report));
    }
    return { agent, children };
  };
  return agents.filter((a) => !a.reportsTo || !ids.has(a.reportsTo)).map(nodeOf);
}

/** Todos abaixo de `id`, em qualquer nível: a quem ele não pode passar a reportar. */
export function descendantsOf(agents: OrgAgent[], id: string): Set<string> {
  const below = new Set<string>();
  const queue = [id];
  while (queue.length > 0) {
    const current = queue.pop()!;
    for (const a of agents) {
      if (a.reportsTo === current && !below.has(a.id)) {
        below.add(a.id);
        queue.push(a.id);
      }
    }
  }
  return below;
}

/** A quem `id` pode passar a reportar: todos menos ele mesmo e quem está abaixo dele (fecharia um ciclo). */
export function superiorChoices<T extends OrgAgent>(agents: T[], id: string): T[] {
  const below = descendantsOf(agents, id);
  return agents.filter((a) => a.id !== id && !below.has(a.id));
}

// Uma mudança feita pela árvore, como o hub a recebe (`AgentOrgEdit`): o tipo mora com o protocolo, em `protocol/messages.ts`.
export type { OrgEdit };

/** Um campo em branco é nenhum valor: a mudança o deixa de fora em vez de mandar uma string vazia. */
export function positionEdit(id: string, role: string, reportsTo: string): OrgEdit {
  return { kind: "setPosition", id, ...(role.trim() ? { role: role.trim() } : {}), ...(reportsTo ? { reportsTo } : {}) };
}

/** P122 — os tipos de ação que um agente pode ser obrigado a ter aprovados mesmo quando age sozinho. Os ids são os de
 * `warden_core::autonomy::Category`, como o hub e `approvalRequired` os escrevem (a mesma lista da web). */
export const APPROVAL_CATEGORIES: { id: string; label: string }[] = [
  { id: "delete_data", label: "Apagar dados" },
  { id: "spend_money", label: "Gastar dinheiro" },
  { id: "critical_infra", label: "Mexer em infraestrutura crítica (shell, SSH, nós)" },
  { id: "external_message", label: "Enviar mensagem externa (navegador, serviços de fora)" },
  { id: "publish_code", label: "Publicar código" },
  { id: "important_config", label: "Alterar configuração importante (tarefas agendadas)" },
  { id: "elevated_agent", label: "Criar ou alterar agentes" },
];

/** O nome de uma categoria, ou o próprio id quando um hub mais novo manda uma que esta tela não conhece. */
export function approvalCategoryLabel(id: string): string {
  return APPROVAL_CATEGORIES.find((c) => c.id === id)?.label ?? id;
}

/** O que cada nível de autonomia quer dizer, em uma linha (P122). */
export const AUTONOMY_LEVELS: { level: number; label: string }[] = [
  { level: 1, label: "1 — só responde, sem usar ferramentas" },
  { level: 2, label: "2 — sugere: lê, mas recusa o resto com uma sugestão" },
  { level: 3, label: "3 — pede o seu sim antes de qualquer mudança" },
  { level: 4, label: "4 — age sozinho (as categorias marcadas ainda pedem o sim)" },
  { level: 5, label: "5 — também gerencia subordinados sozinho" },
];

export interface Powers {
  autonomy: number;
  canStartTasks: boolean;
  canCreateWorkers: boolean;
  canMessageUser: boolean;
  canChooseModels: boolean;
  /** Os ids marcados, em qualquer ordem. */
  approvalRequired: Iterable<string>;
}

/** P122 — a mudança de poderes de `id`. As categorias saem na ordem fixa da lista (as marcadas que a tela conhece, e as que só o hub conhece
 * depois delas), sem repetir, para o mesmo conjunto dar sempre a mesma mudança. */
export function powersEdit(id: string, powers: Powers): OrgEdit {
  const marked = new Set(powers.approvalRequired);
  const known = APPROVAL_CATEGORIES.map((c) => c.id).filter((c) => marked.has(c));
  const unknown = [...marked].filter((c) => !APPROVAL_CATEGORIES.some((k) => k.id === c));
  return {
    kind: "setPowers",
    id,
    autonomy: powers.autonomy,
    canStartTasks: powers.canStartTasks,
    canCreateWorkers: powers.canCreateWorkers,
    canMessageUser: powers.canMessageUser,
    canChooseModels: powers.canChooseModels,
    approvalRequired: [...known, ...unknown],
  };
}

export function addReportEdit(id: string, persona: string, role: string, reportsTo: string | null): OrgEdit {
  return { kind: "addReport", id: id.trim(), persona: persona.trim(), ...(role.trim() ? { role: role.trim() } : {}), ...(reportsTo ? { reportsTo } : {}) };
}

/** Soltar o cartão de `draggedId` sobre o de `targetId` (ou `null`: o topo da árvore): a mudança de posição que o hub recebe, com o cargo que
 * ele já tem. `null` quando não há o que mudar: o agente não existe, o alvo é ele mesmo ou alguém abaixo dele (fecharia um ciclo, o hub também
 * recusa) ou o superior de hoje já é o alvo. */
export function moveEdit(agents: OrgAgent[], draggedId: string, targetId: string | null): OrgEdit | null {
  const dragged = agents.find((a) => a.id === draggedId);
  if (!dragged) return null;
  if (targetId !== null && !superiorChoices(agents, draggedId).some((a) => a.id === targetId)) return null;
  if ((dragged.reportsTo ?? null) === targetId) return null;
  return positionEdit(draggedId, dragged.role ?? "", targetId ?? "");
}

/** Um agente foi renomeado: quem reportava a ele acompanha o nome novo. */
export function renameInReports<T extends OrgAgent>(agents: T[], from: string, to: string): T[] {
  if (from === to) return agents;
  return agents.map((a) => (a.reportsTo === from ? { ...a, reportsTo: to } : a));
}

/** `removed` está saindo: quem reportava a ele passa a reportar ao superior dele (ou a ninguém). A lista volta sem ele. */
export function removeFromOrg<T extends OrgAgent>(agents: T[], removed: string): T[] {
  const superior = agents.find((a) => a.id === removed)?.reportsTo ?? null;
  return agents.filter((a) => a.id !== removed).map((a) => (a.reportsTo === removed ? { ...a, reportsTo: superior } : a));
}
