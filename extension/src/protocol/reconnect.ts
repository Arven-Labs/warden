// The pure half of the extension's automatic reconnect: how long to wait before the next try, whether a failure is worth trying again, and
// whether a freshly started service worker should connect on its own. Pure, so it is tested without a browser (`tests/reconnect.test.mjs`);
// `background/index.ts` owns the timer and the connection.

import type { ConnectionStatus } from "../background/connection";
import type { ConnectionSettings } from "../background/popup_protocol";

/** The wait before the first retry. Each failed try doubles it. */
export const BASE_DELAY_MS = 1_000;
/** The longest wait between two tries: a hub that stays down is asked about once a minute, not more. */
export const MAX_DELAY_MS = 60_000;

/** How long a connection has to hold before the run of failed tries is forgotten. Without it, a hub that accepts and drops at once would reset the
 * wait every time and be knocked at once a second. */
export const STABLE_AFTER_MS = 30_000;

/** How the hub's refusal of the pairing key starts (`authError`, a revoked device, "too many attempts"). The words the person sees; `connection.ts`
 * builds its messages from this, and `retryDelayMs` never retries one that starts with it. */
export const AUTH_REJECTED_PREFIX = "authentication rejected";

/** Whether a failure message is the hub saying no, as opposed to the hub or the network being unreachable. */
export function isAuthRejection(message: string): boolean {
  return message.startsWith(AUTH_REJECTED_PREFIX);
}

/**
 * The wait before try number `attempt` (0 for the first retry): doubles from `BASE_DELAY_MS` up to `MAX_DELAY_MS`, then the jitter cuts
 * between none and half of it off, so devices that lost the same hub together do not all knock at the same instant. Never zero. `random`
 * is `Math.random` outside the tests.
 */
export function nextDelayMs(attempt: number, random: () => number = Math.random): number {
  const tries = Number.isFinite(attempt) && attempt > 0 ? Math.floor(attempt) : 0;
  // 2 ** tries overflows to Infinity for a huge `tries`, and Math.min then still gives the ceiling.
  const ceiling = Math.min(MAX_DELAY_MS, BASE_DELAY_MS * 2 ** tries);
  return ceiling / 2 + (ceiling / 2) * random();
}

/**
 * How long to wait before trying again after `status`, or `null` for "do not". Only a failure is tried again, only while the person wants to be
 * connected (they did not disconnect), and never a refusal of the key: the hub counts tries per address, so insisting on a wrong key would
 * lock the person out.
 */
export function retryDelayMs(wanted: boolean, status: ConnectionStatus, attempt: number, random: () => number = Math.random): number | null {
  if (!wanted || status.kind !== "failure" || isAuthRejection(status.message)) return null;
  return nextDelayMs(attempt, random);
}

/** Whether a service worker that has just started should connect by itself: the person was connected when it went away, and the address it
 * dialled is saved. */
export function canAutoConnect(wanted: boolean, saved: Partial<ConnectionSettings>): boolean {
  const { host, port, deviceName } = saved;
  return wanted && !!host && typeof port === "number" && Number.isInteger(port) && port > 0 && port <= 65_535 && !!deviceName;
}
