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
  /** A key rotation stopped half way and was neither finished nor given up: the automatic sync stays paused for it. */
  rotationHalfDone: boolean;
}

/** Whether the section says that the automatic sync is paused by a key change left half way. Only that state: while a key merely waits
 * for its confirmation the rotation block is open on the screen and says what is going on. */
export function rotationHalfDoneShown(status: ModeStatus | null): boolean {
  return status !== null && status.on && status.rotationHalfDone === true;
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
  /** The logins were accepted but their folder is gone: the class was left out of the cycle and nothing was removed. */
  secretFolderMissing: boolean;
  /** Every file of the vault is gone from this computer: the vault was left out of the cycle, nothing was removed elsewhere. */
  vaultEmptied: boolean;
  /** The logins folder has no login left to send: the class was left out of the cycle, nothing was removed elsewhere. */
  secretEmptied: boolean;
}

/** Why the last automatic cycle failed; `authRejected` is the one that stops the loop. */
export interface LoopError {
  kind: "authRejected" | "other";
  message: string;
}

/** What `arca_loop_last` answers: the last automatic cycle, or null while none has run (nothing is kept across a restart). */
export interface LoopStatus {
  /** When the cycle ran, in ms since the epoch. */
  atMs: number;
  report: SyncReport | null;
  error: LoopError | null;
  /** The loop is stopped now (the hub refused the token and nothing signed in again). */
  stopped: boolean;
}

export type LoopErrorView = { key: "arca.loopStopped" | "arca.loopFailed"; message: string };

/** What the section says about the last automatic cycle: its notices (only when there is something to say, so a clean cycle clears
 * an old warning) and its error. */
export function loopView(status: LoopStatus | null): { notices: ReturnType<typeof noticesOf> | null; error: LoopErrorView | null } {
  if (status === null) return { notices: null, error: null };
  const notices = status.report ? noticesOf(status.report) : null;
  const hasNotice = notices !== null && (notices.counts.length > 0 || notices.skipped.length > 0 || notices.folderMissing || notices.vaultEmptied || notices.loginsEmptied);
  const error: LoopErrorView | null = status.error ? { key: status.error.kind === "authRejected" ? "arca.loopStopped" : "arca.loopFailed", message: status.error.message } : null;
  return { notices: hasNotice ? notices : null, error };
}

/** What one integrity round of the copy the hub holds found: counts only. `damaged` is corrupt and unreadable together (the Warden hub
 * answers 500 to a blob whose bytes changed, so the two cannot be told apart). */
export interface VerifyCounts {
  manifestsOk: number;
  rejected: number;
  referencedBlobs: number;
  missing: number;
  damaged: number;
  sampled: number;
}

/** What `arca_verify_last` answers: the last finished round (from this launch, or remembered on disk, then without counts), or null
 * while none ever ran. A failure is shown beside the last finished round, not in place of it. */
export interface VerifyStatus {
  /** When the last finished round ran, in ms since the epoch; 0 when none ever finished. */
  atMs: number;
  clean: boolean;
  counts: VerifyCounts | null;
  error: LoopError | null;
  stopped: boolean;
}

export type VerifyView =
  | { key: "arca.verifyClean"; params: { date: string } }
  | { key: "arca.verifyProblem"; params: { date: string; missing: number; damaged: number; rejected: number } }
  | { key: "arca.verifyProblemNoCounts"; params: { date: string } }
  | { key: "arca.verifyNever"; params: Record<string, never> };

/** What the section says about the integrity of the copy: the last finished round, in words. `formatDate` is the screen's own. */
export function verifyView(status: VerifyStatus | null, formatDate: (ms: number) => string): VerifyView | null {
  if (status === null) return null;
  if (status.atMs === 0) return { key: "arca.verifyNever", params: {} };
  const date = formatDate(status.atMs);
  if (status.clean) return { key: "arca.verifyClean", params: { date } };
  if (status.counts === null) return { key: "arca.verifyProblemNoCounts", params: { date } };
  return { key: "arca.verifyProblem", params: { date, missing: status.counts.missing, damaged: status.counts.damaged, rejected: status.counts.rejected } };
}

/** The failure of the last round, if there is one worth saying. A refused token says nothing here: it stops the sync cycle too, and
 * that banner (`arca.loopStopped`) already tells the person to sign in again. */
export function verifyErrorMessage(status: VerifyStatus | null): string | null {
  if (status === null || status.error === null || status.error.kind === "authRejected") return null;
  return status.error.message;
}

/** A copy of the loser of a conflict (a login in clear text) that `arca_losers_list` answers: never its text nor its absolute path.
 * `id` only goes back to `arca_losers_clear`. */
export interface LoserItem {
  id: string;
  class: "config" | "secret";
  /** The file that was in conflict (the login's file name). */
  path: string;
  /** The device whose version lost. */
  device: string;
  shaPrefix: string;
  /** The person edited the copy: the arca never deletes it. */
  edited: boolean;
  sizeBytes: number | null;
  modifiedAtMs: number | null;
}

/** A manifest of a newer key epoch that stops the vault (arca ADR 0018), as `arca_epochs_pending` answers. The header of the epoch is
 * not authenticated: anyone with the old key can forge one. `id` is `manifest/<64 hex>` and goes back, with the epoch, to
 * `arca_epochs_acknowledge` as a pair: the epoch alone would let a forged object hide the real rotation. */
export interface PendingEpoch {
  epoch: number;
  id: string;
}

/** What `arca_rotation_begin` answers (P-49): the 24 words of the new key's phrase, shown once, and which of them (counting from 1) the
 * person types back before anything is written. The phrase is never stored: only the component that shows it holds it. */
export interface RotationStart {
  phrase: string[];
  positions: number[];
}

/** The first hex digits of the object id, enough for a person to tell two apart (never used to look anything up). */
export function epochShortId(id: string): string {
  return id.slice(id.indexOf("/") + 1).slice(0, 12);
}

/** What `arca_losers_clear` answers. */
export interface ClearReport {
  removed: number;
  /** Each copy asked for that was not deleted, with the arca's reason. */
  skipped: { id: string; reason: "edited" | "notACopy" | "gone" | "failed" }[];
  /** Work files that may still hold a login and are not in the list: only the count. */
  keptTmp: number;
}

export type LoserSkipKey = "arca.loserSkipEdited" | "arca.loserSkipNotACopy" | "arca.loserSkipGone" | "arca.loserSkipFailed";

/** What the section says after deleting copies: how many went, which stayed and why (translated), and whether a work file may still
 * hold a login. */
export function clearSummary(report: ClearReport): { removed: number; notRemoved: { id: string; reasonKey: LoserSkipKey }[]; keptTmp: number } {
  const reasons: Record<ClearReport["skipped"][number]["reason"], LoserSkipKey> = {
    edited: "arca.loserSkipEdited",
    notACopy: "arca.loserSkipNotACopy",
    gone: "arca.loserSkipGone",
    failed: "arca.loserSkipFailed",
  };
  return { removed: report.removed, notRemoved: report.skipped.map((s) => ({ id: s.id, reasonKey: reasons[s.reason] })), keptTmp: report.keptTmp };
}

/** A file the arca set aside when an interrupted update could not put it back (`.arca-kept-*`), as `arca_kept_list` answers it: never its
 * content nor its absolute path. It is a file of the person, and its own name (a hash) says nothing, so the folder is what tells what it
 * is (empty: the class's folder itself). `id` only goes back to `arca_kept_clear`. */
export interface KeptItem {
  id: string;
  class: "vault" | "config" | "secret";
  folder: string;
  shaPrefix: string;
  /** The person changed the file: the arca never deletes it. */
  edited: boolean;
  sizeBytes: number | null;
  modifiedAtMs: number | null;
}

export type KeptClassKey = "arca.keptClassVault" | "arca.keptClassConfig" | "arca.keptClassSecret";
export type KeptSkipKey = "arca.keptSkipEdited" | "arca.keptSkipNotACopy" | "arca.keptSkipGone" | "arca.keptSkipFailed";

/** The words for the folder a kept file belongs to. */
export function keptClassKey(item: KeptItem): KeptClassKey {
  const keys: Record<KeptItem["class"], KeptClassKey> = { vault: "arca.keptClassVault", config: "arca.keptClassConfig", secret: "arca.keptClassSecret" };
  return keys[item.class];
}

/** What the section says after deleting a kept file: the same report as for the copies (`ClearReport`), told with words about files. */
export function keptClearSummary(report: ClearReport): { removed: number; notRemoved: { id: string; reasonKey: KeptSkipKey }[]; keptTmp: number } {
  const reasons: Record<ClearReport["skipped"][number]["reason"], KeptSkipKey> = {
    edited: "arca.keptSkipEdited",
    notACopy: "arca.keptSkipNotACopy",
    gone: "arca.keptSkipGone",
    failed: "arca.keptSkipFailed",
  };
  return { removed: report.removed, notRemoved: report.skipped.map((s) => ({ id: s.id, reasonKey: reasons[s.reason] })), keptTmp: report.keptTmp };
}

/** The ids "delete all" sends: every copy but the ones the person edited (the arca would refuse those anyway, and the screen says so). */
export function deletableIds(items: LoserItem[]): string[] {
  return items.filter((item) => !item.edited).map((item) => item.id);
}

/** A size for a person: bytes below 1 KB, then one decimal; a dash when it is unknown. */
export function sizeLabel(bytes: number | null): string {
  if (bytes === null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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

/** The classes the person can empty on purpose, as the backend names them (`arca_sync_allow_empty`). */
export type EmptyableClass = "vault" | "secret";

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
  folderMissing: boolean;
  vaultEmptied: boolean;
  loginsEmptied: boolean;
} {
  const counts: { key: NoticeKey; count: number }[] = [
    { key: "arca.noticeIgnored", count: report.secretIgnored },
    { key: "arca.noticeKept", count: report.recoveredKept },
    { key: "arca.noticeLosers", count: report.secretLosersSaved },
  ];
  return {
    counts: counts.filter((c) => c.count > 0),
    skipped: report.secretSkipped.map((s) => ({ path: s.path, reasonKey: SKIP_REASONS.get(s.reason) ?? null, reason: s.reason })),
    folderMissing: report.secretFolderMissing,
    vaultEmptied: report.vaultEmptied,
    loginsEmptied: report.secretEmptied,
  };
}

/** The sentence a finished cycle gets, as a catalog key and its values; `conflicts` is told apart, to be shown beside it. */
export function summaryOf(report: SyncReport): { key: "arca.syncNothing" | "arca.syncDone"; values?: { received: number; sent: number }; conflicts: number } {
  const nothing = !report.snapshotCreated && report.pulled === 0 && report.merges === 0 && report.filesChanged === 0 && report.pushed === 0;
  if (nothing) return { key: "arca.syncNothing", conflicts: report.conflicts };
  return { key: "arca.syncDone", values: { received: report.filesChanged, sent: report.pushed }, conflicts: report.conflicts };
}
