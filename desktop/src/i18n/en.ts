// P136 — every text of the app, in English. One file per part of the app under `en/`; `pt-BR.ts` has the same keys.
import { access } from "./en/access.ts";
import { agents } from "./en/agents.ts";
import { automation } from "./en/automation.ts";
import { chat } from "./en/chat.ts";
import { hubs } from "./en/hubs.ts";
import { lib } from "./en/lib.ts";
import { org } from "./en/org.ts";
import { projects } from "./en/projects.ts";
import { settings } from "./en/settings.ts";
import { shell } from "./en/shell.ts";
import { skills } from "./en/skills.ts";
import { sync } from "./en/sync.ts";
import { usage } from "./en/usage.ts";
import { vault } from "./en/vault.ts";
import { workspace } from "./en/workspace.ts";

export const en = { ...access, ...agents, ...automation, ...chat, ...hubs, ...lib, ...org, ...projects, ...settings, ...shell, ...skills, ...sync, ...usage, ...vault, ...workspace } as const;

export type Key = keyof typeof en;
