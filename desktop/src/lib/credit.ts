import type { Credit } from "../types";

// The hosted credit as the usage screen shows it. Kept out of the component so the rules are testable without a DOM.

const MICROS_PER_USD = 1_000_000;

/** Micro-USD → dollars, for display only (the hub keeps the exact integer). */
export function creditUsd(micros: number): number {
  return micros / MICROS_PER_USD;
}

/** Dollars as the usage screen and the chat header write them: more decimals under one dollar. */
export function formatUsd(value: number): string {
  return `$${value.toFixed(value < 1 ? 4 : 2)}`;
}

/** No credit left for a new call: what is available (balance minus holds) is zero or less. */
export function isOutOfCredit(credit: Credit): boolean {
  return credit.availableMicroUsd <= 0;
}

/** The address to buy more credit, if the hub sent a usable one: it is handed to the system browser, so only a plain `https://` address
 * with no login in it passes (a hub is trusted with the chat, not with opening a `file:` or `javascript:` link). Anything else reads as
 * "no link", and the balance chip and the error button open the usage screen instead. */
export function topUpUrl(raw: string | undefined | null): string | null {
  if (!raw) return null;
  try {
    const url = new URL(raw);
    return url.protocol === "https:" && url.hostname !== "" && url.username === "" && url.password === "" ? url.href : null;
  } catch {
    return null;
  }
}

/** Calls in progress hold part of the balance; only then is "available" worth showing next to it. */
export function hasHolds(credit: Credit): boolean {
  return credit.availableMicroUsd !== credit.balanceMicroUsd;
}
