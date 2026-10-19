// P123 — o limite de modelos de um agente e as políticas nomeadas ("fast", "reasoning"...), como a extensão os mostra e os edita. Puro.
// A extensão manda cada mudança pela operação estreita do hub (`setDelegationModels`, `setModelPolicies`), sem o formulário inteiro de
// configurações; a validação de verdade é a do hub.

import { t } from "../../i18n/index.ts";
import type { ModelPolicy, OrgEdit } from "../../protocol/messages";

/** O que a lista diz sobre os modelos de um agente, numa frase que o cartão mostra embaixo. */
export function delegationSummary(models: string[] | undefined): string {
  if (!models || models.length === 0) return t("policy.open");
  if (models.length === 1) return t("policy.dictated", { model: models[0] });
  return t("policy.limited", { count: models.length, model: models[0] });
}

/** Todo id a que um agente pode ser limitado: os provedores e combos, depois as políticas. */
export function delegationCandidates(modelIds: string[], policies: ModelPolicy[]): string[] {
  return [...modelIds, ...policies.map((p) => p.id)].filter((id) => id.trim() !== "");
}

/** A lista que o formulário do limite manda: o padrão primeiro (se está entre os marcados), depois os outros na ordem dos candidatos.
 * Nada marcado deixa a escolha aberta. */
export function limitModels(candidates: string[], checked: Set<string>, defaultModel: string): string[] {
  const chosen = candidates.filter((id) => checked.has(id));
  if (!checked.has(defaultModel)) return chosen;
  return [defaultModel, ...chosen.filter((id) => id !== defaultModel)];
}

export function limitEdit(agentId: string, models: string[]): OrgEdit {
  return { kind: "setDelegationModels", id: agentId, models };
}

/** A lista de políticas depois de salvar `policy` (que tinha o nome `originalId`, ou nenhum se é nova): troca a que existia, no mesmo lugar,
 * ou acrescenta no fim. O nome e a descrição saem aparados. */
export function policiesWith(policies: ModelPolicy[], policy: ModelPolicy, originalId: string | null): ModelPolicy[] {
  const clean: ModelPolicy = { id: policy.id.trim(), model: policy.model.trim(), description: (policy.description ?? "").trim() };
  const at = originalId === null ? -1 : policies.findIndex((p) => p.id === originalId);
  if (at < 0) return [...policies, clean];
  return policies.map((p, i) => (i === at ? clean : p));
}

export function policiesWithout(policies: ModelPolicy[], id: string): ModelPolicy[] {
  return policies.filter((p) => p.id !== id);
}

/** Como o hub recebe as políticas: a descrição sempre presente (vazia quando não há). */
export function policiesEdit(policies: ModelPolicy[]): OrgEdit {
  return { kind: "setModelPolicies", policies: policies.map((p) => ({ id: p.id, model: p.model, description: p.description ?? "" })) };
}

/** O nome com que uma política nova começa: `policy-1`, `policy-2`... livre entre provedores, combos e políticas. */
export function nextPolicyId(taken: string[]): string {
  let n = 1;
  while (taken.includes(`policy-${n}`)) n += 1;
  return `policy-${n}`;
}
