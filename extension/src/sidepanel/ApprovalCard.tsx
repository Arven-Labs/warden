import type { ApprovalPrompt } from "../protocol/messages";
import { t } from "../i18n/index.ts";

interface Props {
  prompt: ApprovalPrompt;
  /** How many more are queued behind this one. */
  waiting: number;
}

/** P87 — a tool in this device's turn waits on the person (an agent creating or editing another, an
 * SSH host with approval, a spending-limit pause). The hub gives up after 120 s, which removes it. */
export default function ApprovalCard({ prompt, waiting }: Props) {
  function answer(approved: boolean) {
    void chrome.runtime.sendMessage({ type: "resolveApproval", approvalId: prompt.approvalId, approved });
  }

  return (
    <section className="approval-card" role="alertdialog" aria-label={t("approval.asked")}>
      <strong>{t("approval.asked")}</strong>
      <p>
        {prompt.action}: <code>{prompt.target}</code>
      </p>
      {prompt.category && <p className="approval-detail">{t("approval.category", { category: prompt.category })}</p>}
      {prompt.detail && <p className="approval-detail">{prompt.detail}</p>}
      {waiting > 0 && <p className="approval-detail">{t("approval.more", { count: waiting })}</p>}
      <div className="approval-actions">
        <button type="button" onClick={() => answer(true)}>
          {t("approval.approve")}
        </button>
        <button type="button" className="link-button" onClick={() => answer(false)}>
          {t("approval.refuse")}
        </button>
      </div>
    </section>
  );
}
