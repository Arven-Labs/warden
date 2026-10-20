// P136 — the same texts in Brazilian Portuguese: `tsc` names any key of `en.ts` that is missing here.
import type { Key } from "./en.ts";
import { accessPt } from "./pt-BR/access.ts";
import { agentsPt } from "./pt-BR/agents.ts";
import { automationPt } from "./pt-BR/automation.ts";
import { chatPt } from "./pt-BR/chat.ts";
import { hubsPt } from "./pt-BR/hubs.ts";
import { libPt } from "./pt-BR/lib.ts";
import { orgPt } from "./pt-BR/org.ts";
import { projectsPt } from "./pt-BR/projects.ts";
import { settingsPt } from "./pt-BR/settings.ts";
import { shellPt } from "./pt-BR/shell.ts";
import { skillsPt } from "./pt-BR/skills.ts";
import { syncPt } from "./pt-BR/sync.ts";
import { usagePt } from "./pt-BR/usage.ts";
import { vaultPt } from "./pt-BR/vault.ts";
import { workspacePt } from "./pt-BR/workspace.ts";

export const ptBR: Record<Key, string> = { ...accessPt, ...agentsPt, ...automationPt, ...chatPt, ...hubsPt, ...libPt, ...orgPt, ...projectsPt, ...settingsPt, ...shellPt, ...skillsPt, ...syncPt, ...usagePt, ...vaultPt, ...workspacePt };
