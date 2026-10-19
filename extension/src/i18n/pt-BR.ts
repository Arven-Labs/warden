// P136 — the same texts in Brazilian Portuguese: `tsc` names any key of `en.ts` that is missing here.
import type { Key } from "./en.ts";
import { backgroundPt } from "./pt-BR/background.ts";
import { helpersPt } from "./pt-BR/helpers.ts";
import { orgPt } from "./pt-BR/org.ts";
import { shellPt } from "./pt-BR/shell.ts";
import { skillsPt } from "./pt-BR/skills.ts";
import { viewsPt } from "./pt-BR/views.ts";

export const ptBR: Record<Key, string> = { ...backgroundPt, ...helpersPt, ...orgPt, ...shellPt, ...skillsPt, ...viewsPt };
