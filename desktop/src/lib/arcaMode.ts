// Warden P183 — the Sync screen's side of the arca mode of the vault: what `arca_mode_status` says becomes which controls to draw,
// and what a sync cycle reports becomes one sentence. The backend is `arca_mode_cmds.rs`.

/** What `arca_mode_status` answers. */
export interface ModeStatus {
  hasKey: boolean;
  on: boolean;
  pendingSend: boolean;
  /** The saved hub the automatic sync uses; null until the person chooses one. May name a hub that was removed since. */
  hubId: string | null;
  /** The person accepted that the MCP logins travel; until then they are neither sent nor applied. */
  loginsAccepted: boolean;
}

/** What the section says about the MCP logins: nothing before the mode is on (the choice is a box in the confirmation); once on,
 * either that they are not syncing yet (with the offer to take them, the case of a mode turned on before the consent existed) or
 * that they are. */
export function loginsView(status: ModeStatus | null): "hidden" | "offer" | "syncing" {
  if (status === null || !status.on) return "hidden";
  return status.loginsAccepted ? "syncing" : "offer";
}

/** What the hub selector shows: the recorded hub if it is still saved (never "the first of the list", which would pretend a choice
 * was made), and whether the recorded one is gone. Nothing to warn about until the list of saved hubs has arrived. */
export function hubChoice(recorded: string | null, hubs: { id: string }[] | null): { selected: string; removed: boolean } {
  if (recorded === null || hubs === null) return { selected: "", removed: false };
  return hubs.some((hub) => hub.id === recorded) ? { selected: recorded, removed: false } : { selected: "", removed: true };
}

/** What `arca_sync_run` answers. */
export interface SyncReport {
  snapshotCreated: boolean;
  pulled: number;
  merges: number;
  conflicts: number;
  filesChanged: number;
  pushed: number;
  secretLosersSaved: number;
  secretIgnored: number;
  /** Empty also when the cycle made no snapshot: empty is not "nothing was skipped". */
  secretSkipped: { path: string; reason: string }[];
  recoveredKept: number;
}

/** Which controls the section draws. */
export type ModeView = "loading" | "needsKey" | "pendingSend" | "ready" | "on";

/** Once the mode is on nothing else matters (there is no way back); before that, a missing key comes first, because a Send can wait
 * only on a computer that has one. */
export function viewOf(status: ModeStatus | null): ModeView {
  if (status === null) return "loading";
  if (status.on) return "on";
  if (!status.hasKey) return "needsKey";
  if (status.pendingSend) return "pendingSend";
  return "ready";
}

export type NoticeKey = "arca.noticeIgnored" | "arca.noticeKept" | "arca.noticeLosers";
export type SkipKey = "arca.skipNotAccepted" | "arca.skipReserved" | "arca.skipCase";

// The reasons `arca-core` gives for leaving a login out, in its own words (the screen translates the ones it knows).
const SKIP_REASONS = new Map<string, SkipKey>([
  ["not accepted by this device", "arca.skipNotAccepted"],
  ["reserved name", "arca.skipReserved"],
  ["differs only by case from another file", "arca.skipCase"],
]);

/** What a cycle says about the logins, apart from the sentence: counters that are not zero, and each file left out with its reason.
 * `reasonKey` is null for a reason this screen does not know, so the screen shows `reason` as it came instead of hiding it. */
export function noticesOf(report: SyncReport): {
  counts: { key: NoticeKey; count: number }[];
  skipped: { path: string; reasonKey: SkipKey | null; reason: string }[];
} {
  const counts: { key: NoticeKey; count: number }[] = [
    { key: "arca.noticeIgnored", count: report.secretIgnored },
    { key: "arca.noticeKept", count: report.recoveredKept },
    { key: "arca.noticeLosers", count: report.secretLosersSaved },
  ];
  return {
    counts: counts.filter((c) => c.count > 0),
    skipped: report.secretSkipped.map((s) => ({ path: s.path, reasonKey: SKIP_REASONS.get(s.reason) ?? null, reason: s.reason })),
  };
}

/** The sentence a finished cycle gets, as a catalog key and its values; `conflicts` is told apart, to be shown beside it. */
export function summaryOf(report: SyncReport): { key: "arca.syncNothing" | "arca.syncDone"; values?: { received: number; sent: number }; conflicts: number } {
  const nothing = !report.snapshotCreated && report.pulled === 0 && report.merges === 0 && report.filesChanged === 0 && report.pushed === 0;
  if (nothing) return { key: "arca.syncNothing", conflicts: report.conflicts };
  return { key: "arca.syncDone", values: { received: report.filesChanged, sent: report.pushed }, conflicts: report.conflicts };
}
