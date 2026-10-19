// P136 — every text of the extension, in English. One file per part under `en/`; `pt-BR.ts` has the same keys.
import { background } from "./en/background.ts";
import { helpers } from "./en/helpers.ts";
import { org } from "./en/org.ts";
import { shell } from "./en/shell.ts";
import { skills } from "./en/skills.ts";
import { views } from "./en/views.ts";

export const en = { ...background, ...helpers, ...org, ...shell, ...skills, ...views } as const;

export type Key = keyof typeof en;
