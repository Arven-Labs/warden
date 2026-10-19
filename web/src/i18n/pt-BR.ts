// P136 — the same texts in Brazilian Portuguese: `tsc` names any key of `en.ts` that is missing here.
import type { Key } from "./en.ts";
import { accountPt } from "./pt-BR/account.ts";
import { adminPt } from "./pt-BR/admin.ts";
import { appPt } from "./pt-BR/app.ts";
import { authPt } from "./pt-BR/auth.ts";
import { chatPt } from "./pt-BR/chat.ts";
import { commonPt } from "./pt-BR/common.ts";
import { devicesPt } from "./pt-BR/devices.ts";
import { dialogsPt } from "./pt-BR/dialogs.ts";
import { hubPt } from "./pt-BR/hub.ts";
import { machinePt } from "./pt-BR/machine.ts";
import { nodesPt } from "./pt-BR/nodes.ts";
import { orgPt } from "./pt-BR/org.ts";
import { peoplePt } from "./pt-BR/people.ts";
import { projectsPt } from "./pt-BR/projects.ts";
import { settingsPt } from "./pt-BR/settings.ts";
import { skillsPt } from "./pt-BR/skills.ts";
import { syncPt } from "./pt-BR/sync.ts";
import { usagePt } from "./pt-BR/usage.ts";
import { vaultPt } from "./pt-BR/vault.ts";
import { webhooksPt } from "./pt-BR/webhooks.ts";

export const ptBR: Record<Key, string> = { ...accountPt, ...adminPt, ...appPt, ...authPt, ...chatPt, ...commonPt, ...devicesPt, ...dialogsPt, ...hubPt, ...machinePt, ...nodesPt, ...orgPt, ...peoplePt, ...projectsPt, ...settingsPt, ...skillsPt, ...syncPt, ...usagePt, ...vaultPt, ...webhooksPt };
