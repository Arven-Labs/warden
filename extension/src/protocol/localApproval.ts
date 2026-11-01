// The yes the person gives, in the side panel, before `browser_type_text` writes anything. The hub's own approvals (`ApprovalPrompt`, P87) are asked
// by the hub; this one is asked by the extension, so it holds for every agent and every autonomy level without touching the hub's core. It fails
// closed: no answer in time, a connection that dropped or an id that is not waiting never becomes a yes. Pure, so it is tested without a browser
// (`tests/localApproval.test.mjs`); `background/index.ts` shows the questions in the panel's approval card and relays the answers.

import type { ApprovalPrompt } from "./messages";

/** Local questions have negative ids; the hub's are 0 or more, so an answer can be told apart by its id alone. */
export function isLocalApprovalId(approvalId: number): boolean {
  return Number.isInteger(approvalId) && approvalId < 0;
}

type Waiting = {
  prompt: ApprovalPrompt;
  settle: (approved: boolean) => void;
  timer: ReturnType<typeof setTimeout>;
};

export class LocalApprovals {
  private nextId = -1;
  private readonly waiting = new Map<number, Waiting>();

  private readonly onChange: () => void;

  /** `onChange` runs whenever the list of waiting questions changes, so the panel can be told. */
  constructor(onChange: () => void = () => {}) {
    this.onChange = onChange;
  }

  /** The questions still waiting for the person, oldest first. */
  pending(): ApprovalPrompt[] {
    return [...this.waiting.values()].map((w) => w.prompt);
  }

  /** Asks the person. Resolves `true` only on a yes; a no, `timeoutMs` without an answer, or `denyAll` resolve `false`. */
  request(prompt: Omit<ApprovalPrompt, "approvalId">, timeoutMs: number): Promise<boolean> {
    if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) return Promise.resolve(false);
    return new Promise<boolean>((resolve) => {
      const approvalId = this.nextId--;
      const timer = setTimeout(() => this.settle(approvalId, false), timeoutMs);
      // In Node (the tests) an unanswered question must not keep the process alive; a browser's timer id has no `unref`.
      (timer as { unref?: () => void }).unref?.();
      this.waiting.set(approvalId, { prompt: { ...prompt, approvalId }, settle: resolve, timer });
      this.onChange();
    });
  }

  /** The person's answer. `false` when no such question is waiting (already answered, timed out, or an id that was never ours). */
  resolve(approvalId: number, approved: boolean): boolean {
    if (!this.waiting.has(approvalId)) return false;
    this.settle(approvalId, approved);
    return true;
  }

  /** The answer could not be delivered (the connection dropped): every question is a no. */
  denyAll(): void {
    for (const approvalId of [...this.waiting.keys()]) this.settle(approvalId, false);
  }

  private settle(approvalId: number, approved: boolean): void {
    const entry = this.waiting.get(approvalId);
    if (!entry) return;
    clearTimeout(entry.timer);
    this.waiting.delete(approvalId);
    entry.settle(approved);
    this.onChange();
  }
}
