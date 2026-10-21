// P121 — the agents that can start messages (`message_user`, `[[outreach]]`) and the external channels they are also forwarded to, as the
// Settings form edits them. Pure; mirrors `desktop/src/lib/outreach.ts`.

import type { OutreachEntry } from "./messages";

/** The external channels a message started by the agent can also go to. */
export const OUTREACH_CHANNELS = ["telegram", "whatsapp"] as const;

export type OutreachChannel = (typeof OUTREACH_CHANNELS)[number];

/** Whether the agent can start messages. */
export function outreachOn(list: OutreachEntry[], agent: string): boolean {
  return list.some((o) => o.agent === agent);
}

/** An agent's external channels (empty if it cannot start messages). */
export function outreachForward(list: OutreachEntry[], agent: string): string[] {
  return list.find((o) => o.agent === agent)?.forward ?? [];
}

/** Turns the agent on or off. Turning on starts with no external channel; turning off forgets its channels. */
export function setOutreach(list: OutreachEntry[], agent: string, on: boolean): OutreachEntry[] {
  if (on) return outreachOn(list, agent) ? list : [...list, { agent, forward: [] }];
  return list.filter((o) => o.agent !== agent);
}

/** Turns an external channel on or off for an agent that can start messages; for one that cannot, nothing changes. */
export function setForward(list: OutreachEntry[], agent: string, channel: OutreachChannel, on: boolean): OutreachEntry[] {
  return list.map((o) => {
    if (o.agent !== agent) return o;
    const rest = o.forward.filter((c) => c !== channel);
    return { ...o, forward: on ? OUTREACH_CHANNELS.filter((c) => c === channel || rest.includes(c)) : rest };
  });
}

/** An agent was renamed: its entry follows it. */
export function renameOutreach(list: OutreachEntry[], from: string, to: string): OutreachEntry[] {
  if (from === to) return list;
  return list.map((o) => (o.agent === from ? { ...o, agent: to } : o));
}

/** Um agente vai embora: a entrada dele vai junto. */
export function dropOutreach(list: OutreachEntry[], agent: string): OutreachEntry[] {
  return list.filter((o) => o.agent !== agent);
}
