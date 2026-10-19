// P136 — every text of the page, in English. One file per part of the page under `en/`; `pt-BR.ts` has the same keys.
import { account } from "./en/account.ts";
import { admin } from "./en/admin.ts";
import { app } from "./en/app.ts";
import { auth } from "./en/auth.ts";
import { chat } from "./en/chat.ts";
import { common } from "./en/common.ts";
import { devices } from "./en/devices.ts";
import { dialogs } from "./en/dialogs.ts";
import { hub } from "./en/hub.ts";
import { machine } from "./en/machine.ts";
import { nodes } from "./en/nodes.ts";
import { org } from "./en/org.ts";
import { people } from "./en/people.ts";
import { projects } from "./en/projects.ts";
import { settings } from "./en/settings.ts";
import { skills } from "./en/skills.ts";
import { sync } from "./en/sync.ts";
import { usage } from "./en/usage.ts";
import { vault } from "./en/vault.ts";
import { webhooks } from "./en/webhooks.ts";

export const en = { ...account, ...admin, ...app, ...auth, ...chat, ...common, ...devices, ...dialogs, ...hub, ...machine, ...nodes, ...org, ...people, ...projects, ...settings, ...skills, ...sync, ...usage, ...vault, ...webhooks } as const;

export type Key = keyof typeof en;
