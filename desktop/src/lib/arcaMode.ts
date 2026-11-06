// Warden P183 — the Sync screen's side of the arca mode of the vault: what `arca_mode_status` says becomes which controls to draw,
// and what a sync cycle reports becomes one sentence. The backend is `arca_mode_cmds.rs`.

/** What `arca_mode_status` answers. */
export interface ModeStatus {
  hasKey: boolean;
  on: boolean;
  pendingSend: boolean;
}

/** What `arca_sync_run` answers. */
export interface SyncReport {
  snapshotCreated: boolean;
  pulled: number;
  merges: number;
  conflicts: number;
  filesChanged: number;
  pushed: number;
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

/** The sentence a finished cycle gets, as a catalog key and its values; `conflicts` is told apart, to be shown beside it. */
export function summaryOf(report: SyncReport): { key: "arca.syncNothing" | "arca.syncDone"; values?: { received: number; sent: number }; conflicts: number } {
  const nothing = !report.snapshotCreated && report.pulled === 0 && report.merges === 0 && report.filesChanged === 0 && report.pushed === 0;
  if (nothing) return { key: "arca.syncNothing", conflicts: report.conflicts };
  return { key: "arca.syncDone", values: { received: report.filesChanged, sent: report.pushed }, conflicts: report.conflicts };
}
