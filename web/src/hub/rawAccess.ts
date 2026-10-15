// P131 — the pure half of the "Chamadas diretas" editor of a Warden API key: which providers and combos a key may call
// straight (`provider/<id>` or `combo/<id>`, with no agent in between). The list holds the ids of the hub's config, or "*".

import type { ApiKey } from "./messages";

/** In a key's list: every provider and combo that doesn't reach another machine. */
export const RAW_ALL = "*";

interface ProviderLike {
  id: string;
  kind: string;
}

interface ComboLike {
  id: string;
  providers: string[];
}

export interface RawOption {
  id: string;
  kind: "provider" | "combo";
  /** Reaches another machine (a node's model, another hub, or a combo with one): "*" doesn't cover it, it has to be ticked. */
  far: boolean;
}

/** The ids "*" doesn't cover: a node's model, another hub's (`cloud`) and any combo that has one of them as a member. */
export function farIds(providers: ProviderLike[], combos: ComboLike[]): Set<string> {
  const far = new Set(providers.filter((p) => p.kind === "node" || p.kind === "cloud").map((p) => p.id));
  for (const combo of combos) {
    if (combo.providers.some((id) => far.has(id))) far.add(combo.id);
  }
  return far;
}

/** What the editor offers: every provider, then every combo. */
export function rawOptions(providers: ProviderLike[], combos: ComboLike[]): RawOption[] {
  const far = farIds(providers, combos);
  return [
    ...providers.map((p) => ({ id: p.id, kind: "provider" as const, far: far.has(p.id) })),
    ...combos.map((c) => ({ id: c.id, kind: "combo" as const, far: far.has(c.id) })),
  ];
}

/** `raw` with `id` ticked, or unticked if it was. Keeps the order the ids were ticked in. */
export function toggleRaw(raw: string[], id: string): string[] {
  return raw.includes(id) ? raw.filter((r) => r !== id) : [...raw, id];
}

/** Whether "*" is in the list. */
export function hasAll(raw: string[]): boolean {
  return raw.includes(RAW_ALL);
}

/** One line for the list of keys: what the key may call straight. */
export function rawSummary(raw: string[] | undefined): string {
  const ids = (raw ?? []).filter((r) => r !== RAW_ALL);
  const all = (raw ?? []).includes(RAW_ALL);
  if (all && ids.length === 0) return "Todos os modelos locais";
  if (all) return `Todos os modelos locais e ${ids.join(", ")}`;
  return ids.length === 0 ? "Nenhuma" : ids.join(", ");
}

/** Whether the editor is for this key: a general key of the owner. A key bound to an agent, or a member's, can't call a model directly. */
export function canEditRaw(key: ApiKey, member: boolean): boolean {
  return !member && !key.agentId && !key.user;
}

/** The ids in `raw` the hub no longer has (a provider that was removed): they stay in the list until saved, but the screen says so. */
export function missingIds(raw: string[] | undefined, options: RawOption[]): string[] {
  return (raw ?? []).filter((id) => id !== RAW_ALL && !options.some((o) => o.id === id));
}
