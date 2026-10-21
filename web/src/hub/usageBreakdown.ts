// P126 — the pure half of the token breakdowns of the Usage screen ("By agent", "By provider", "By combo").

import type { UsageByKey } from "./messages";

export interface BreakdownRow {
  /** The key the hub grouped by, or an empty string for the bucket with none. */
  key: string;
  /** What the row is called on screen. */
  label: string;
  tokens: number;
  /** Model calls (assistant messages that carried usage). */
  calls: number;
  /** 0 to 1: this row's tokens against the biggest row, which is the width of its bar. */
  share: number;
}

/** The rows of one breakdown, the biggest consumer first. `emptyLabel` names the bucket with no key; an older hub that
 * sends none gives no rows, and the screen hides the section. */
export function breakdownRows(entries: UsageByKey[] | undefined, emptyLabel: string): BreakdownRow[] {
  const rows = (entries ?? []).map((e) => ({
    key: e.key ?? "",
    label: e.key ?? emptyLabel,
    tokens: e.usage.totalTokens,
    calls: e.messageCount,
    share: 0,
  }));
  rows.sort((a, b) => b.tokens - a.tokens);
  const biggest = rows[0]?.tokens ?? 0;
  return rows.map((r) => ({ ...r, share: biggest > 0 ? r.tokens / biggest : 0 }));
}
