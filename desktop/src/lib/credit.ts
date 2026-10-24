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

/** Calls in progress hold part of the balance; only then is "available" worth showing next to it. */
export function hasHolds(credit: Credit): boolean {
  return credit.availableMicroUsd !== credit.balanceMicroUsd;
}
