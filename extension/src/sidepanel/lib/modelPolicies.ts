// P123 — an agent's model limit and the named policies ("fast", "reasoning"...), as the extension shows and edits them. Pure.
// The extension sends each change through the hub's narrow operation (`setDelegationModels`, `setModelPolicies`), without the whole settings
// form; the real validation is the hub's.

import { t } from "../../i18n/index.ts";
import type { ModelPolicy, OrgEdit } from "../../protocol/messages";

/** What the list says about an agent's models, in a sentence the card shows underneath. */
export function delegationSummary(models: string[] | undefined): string {
  if (!models || models.length === 0) return t("policy.open");
  if (models.length === 1) return t("policy.dictated", { model: models[0] });
  return t("policy.limited", { count: models.length, model: models[0] });
}

/** Every id an agent can be limited to: the providers and combos, then the policies. */
export function delegationCandidates(modelIds: string[], policies: ModelPolicy[]): string[] {
  return [...modelIds, ...policies.map((p) => p.id)].filter((id) => id.trim() !== "");
}

/** The list the limit form sends: the default first (if it is among the checked), then the others in candidate order.
 * Nothing checked leaves the choice open. */
export function limitModels(candidates: string[], checked: Set<string>, defaultModel: string): string[] {
  const chosen = candidates.filter((id) => checked.has(id));
  if (!checked.has(defaultModel)) return chosen;
  return [defaultModel, ...chosen.filter((id) => id !== defaultModel)];
}

export function limitEdit(agentId: string, models: string[]): OrgEdit {
  return { kind: "setDelegationModels", id: agentId, models };
}

/** The policy list after saving `policy` (which had the name `originalId`, or none if it is new): replaces the one that existed, in place,
 * or appends at the end. The name and the description are trimmed. */
export function policiesWith(policies: ModelPolicy[], policy: ModelPolicy, originalId: string | null): ModelPolicy[] {
  const clean: ModelPolicy = { id: policy.id.trim(), model: policy.model.trim(), description: (policy.description ?? "").trim() };
  const at = originalId === null ? -1 : policies.findIndex((p) => p.id === originalId);
  if (at < 0) return [...policies, clean];
  return policies.map((p, i) => (i === at ? clean : p));
}

export function policiesWithout(policies: ModelPolicy[], id: string): ModelPolicy[] {
  return policies.filter((p) => p.id !== id);
}

/** How the hub receives the policies: the description always present (empty when there is none). */
export function policiesEdit(policies: ModelPolicy[]): OrgEdit {
  return { kind: "setModelPolicies", policies: policies.map((p) => ({ id: p.id, model: p.model, description: p.description ?? "" })) };
}

/** The name a new policy starts with: `policy-1`, `policy-2`... free among providers, combos and policies. */
export function nextPolicyId(taken: string[]): string {
  let n = 1;
  while (taken.includes(`policy-${n}`)) n += 1;
  return `policy-${n}`;
}
