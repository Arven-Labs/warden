// P123 — the models an agent can delegate with, and the named policies ("fast", "reasoning"...) over them, as the Settings
// form edits them. Pure; mirrors `desktop/src/lib/modelPolicies.ts`.

import { t } from "../i18n/index.ts";
import type { AgentSettings, ModelPolicy } from "./messages";

/** The part of the form that these cascades read and change. */
export interface PolicyForm {
  agents: AgentSettings[];
  modelPolicies: ModelPolicy[];
}

/** Every id an agent can be limited to: the providers, then the combos, then the policies. */
export function delegationCandidates(providers: { id: string }[], combos: { id: string }[], policies: { id: string }[]): string[] {
  return [...providers, ...combos, ...policies].map((m) => m.id).filter((id) => id.trim() !== "");
}

const renameIn = (list: string[] | undefined, from: string, to: string) => list?.map((m) => (m === from ? to : m));

/** A provider or combo was renamed: the policies it answers follow it, and so do the agents that listed it. */
export function renameModel<T extends PolicyForm>(form: T, from: string, to: string): T {
  if (from === to) return form;
  return {
    ...form,
    modelPolicies: form.modelPolicies.map((p) => (p.model === from ? { ...p, model: to } : p)),
    agents: form.agents.map((a) => (a.delegationModels?.includes(from) ? { ...a, delegationModels: renameIn(a.delegationModels, from, to) } : a)),
  };
}

/** A policy was renamed: the agents that listed it follow it. */
export function renamePolicy<T extends PolicyForm>(form: T, from: string, to: string): T {
  if (from === to) return form;
  return { ...form, agents: form.agents.map((a) => (a.delegationModels?.includes(from) ? { ...a, delegationModels: renameIn(a.delegationModels, from, to) } : a)) };
}

/** A provider or combo goes away: the policies it answered go with it, and every agent's list forgets it, and them. A list that
 * ends up empty leaves the choice open again. */
export function dropModel<T extends PolicyForm>(form: T, id: string): T {
  const gone = new Set([id, ...form.modelPolicies.filter((p) => p.model === id).map((p) => p.id)]);
  return {
    ...form,
    modelPolicies: form.modelPolicies.filter((p) => p.model !== id),
    agents: form.agents.map((a) => (a.delegationModels?.some((m) => gone.has(m)) ? { ...a, delegationModels: a.delegationModels.filter((m) => !gone.has(m)) } : a)),
  };
}

/** A policy goes away: the agents that listed it forget it. */
export function dropPolicy<T extends PolicyForm>(form: T, id: string): T {
  return {
    ...form,
    modelPolicies: form.modelPolicies.filter((p) => p.id !== id),
    agents: form.agents.map((a) => (a.delegationModels?.includes(id) ? { ...a, delegationModels: a.delegationModels.filter((m) => m !== id) } : a)),
  };
}

/** The name a new policy starts with: `policy-1`, `policy-2`... free among providers, combos and policies. */
export function nextPolicyId(taken: string[]): string {
  let n = 1;
  while (taken.includes(`policy-${n}`)) n += 1;
  return `policy-${n}`;
}

/** What the list says about an agent's models, in a sentence the card shows underneath. */
export function delegationSummary(models: string[] | undefined): string {
  if (!models || models.length === 0) return t("policy.open");
  if (models.length === 1) return t("policy.dictated", { model: models[0] });
  return t("policy.limited", { count: models.length, model: models[0] });
}
