import { useState } from "react";
import { t } from "../i18n";

interface Props {
  /** The code, as the hub made it (`ABCD-EFGH-…`). It's only ever shown here, once. */
  code: string;
  /** `true` when it replaces an earlier code, which no longer works. */
  replacing?: boolean;
  onDone: () => void;
}

/** P84 slice 4: the recovery code of a member's encrypted data, shown once. It's the only way back to
 * the data if the password is lost or the owner resets it, so the screen doesn't go away until the
 * person says they wrote it down. */
export default function RecoveryCodeView({ code, replacing = false, onDone }: Props) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
    } catch {
      setCopied(false); // no clipboard here: the code is on screen to write down
    }
  }

  return (
    <div className="login">
      <div className="login-card">
        <div className="login-brand">
          <img src="/favicon.svg" alt="" width={40} height={40} />
          <h1>Warden</h1>
        </div>
        <p className="login-hint">
          {replacing ? t("recovery.newCode") : t("recovery.encrypted")}
          {t("recovery.keepIt")}
        </p>
        <p className="recovery-code" aria-label={t("recovery.codeLabel")}>
          <code>{code}</code>
        </p>
        <button type="button" className="link-button" onClick={() => void copy()}>
          {copied ? t("copy.copied") : t("copy.copy")}
        </button>
        <p className="login-hint">{t("recovery.once")}</p>
        <label className="checkbox-row">
          <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} />
          {t("recovery.savedIt")}
        </label>
        <button type="button" className="primary-button" disabled={!saved} onClick={onDone}>
          {t("recovery.continue")}
        </button>
      </div>
    </div>
  );
}
