import type { ApprovalPrompt } from "../hub/connection";
import { approvalCategoryLabel } from "../hub/approvalCategories";
import { t, type Key } from "../i18n";

interface Props {
  /** Oldest first; the first one is shown. */
  queue: ApprovalPrompt[];
  onAnswer: (approvalId: number, approved: boolean, always?: boolean) => void;
}

const VERB: Record<string, Key> = {
  exec: "approval.verb.exec",
  upload: "approval.verb.upload",
  download: "approval.verb.download",
  create_agent: "approval.verb.create_agent",
  update_agent: "approval.verb.update_agent",
  delete_agent: "approval.verb.delete_agent",
  replace_agent: "approval.verb.replace_agent",
  tool_call: "approval.verb.tool_call",
};

/** Asks the person to approve what the AI wants to do in this browser's turn (P46: creating or
 * changing an agent; SSH hosts that ask first; a spending limit that ran out). Same card as the
 * desktop's `ApprovalModal`. Anything but "Aprovar" is a refusal, and the hub counts no answer as
 * one too, so a stray click can't approve anything. */
export default function ApprovalModal({ queue, onAnswer }: Props) {
  const current = queue[0];
  if (!current) return null;
  // A paused turn (P4) is not the AI asking for something: a limit ran out and the turn waits.
  const spendPause = current.action === "extend_limit";

  return (
    <div className="approval-backdrop" role="dialog" aria-modal="true" aria-labelledby="approval-title">
      <div className="approval-card">
        <h2 id="approval-title" className="approval-title">
          {spendPause
            ? t("approval.spendLimit", { target: current.target })
            : t("approval.aiWants", { verb: VERB[current.action] ? t(VERB[current.action]) : current.action, target: current.target })}
        </h2>
        {current.category && !spendPause && <p className="approval-more">{t("approval.asked", { category: approvalCategoryLabel(current.category) })}</p>}
        <pre className="approval-detail">{current.detail}</pre>
        {queue.length > 1 && <p className="approval-more">{t("approval.more", { count: queue.length - 1 })}</p>}
        <div className="approval-actions">
          <button type="button" className="link-button" onClick={() => onAnswer(current.approvalId, false)} autoFocus>
            {spendPause ? t("approval.stop") : t("approval.refuse")}
          </button>
          <button type="button" className="primary-button" onClick={() => onAnswer(current.approvalId, true)}>
            {spendPause ? t("approval.allowMore") : t("approval.approve")}
          </button>
        </div>
        {current.always && (
          <div className="approval-actions">
            <button type="button" className="link-button" onClick={() => onAnswer(current.approvalId, true, true)} title={t("approval.untilRestart")}>
              {t("approval.always", { what: current.always })}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
