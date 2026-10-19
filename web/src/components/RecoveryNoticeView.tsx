import { useState } from "react";
import { UserError, type ServerConnection } from "../hub/connection";
import type { UserInfo } from "../hub/messages";
import { intlTag, t, type Key } from "../i18n";

interface Props {
  conn: ServerConnection;
  user: UserInfo;
  /** They said yes to the workspace's policy. `recoveryCode`: entering or leaving "consent" made a new one. */
  onAccepted: (recoveryCode?: string) => void;
  /** They've seen the recoveries the owner made. */
  onAcked: () => void;
  /** Not now: the notice comes back at the next sign-in. */
  onLater: () => void;
}

const POLICY_TEXT: Record<string, Key> = {
  private: "notice.private",
  consent: "notice.consent",
  company: "notice.company",
};

function when(atMs: number): string {
  return new Date(atMs).toLocaleString(intlTag());
}

/** P84 fatia 4 parte B: the workspace's recovery policy changed to a weaker one and needs their yes, and/or the
 * owner recovered their data. Shown after signing in, before the app. */
export default function RecoveryNoticeView({ conn, user, onAccepted, onAcked, onLater }: Props) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const policy = user.recoveryPolicy ?? "private";
  const unseen = (user.recoveries ?? []).filter((r) => !r.seen);

  async function accept(e: React.FormEvent) {
    e.preventDefault();
    if (busy || !password) return;
    setBusy(true);
    setError(null);
    try {
      const result = await conn.acceptRecoveryPolicy(password);
      onAccepted(result.recoveryCode);
    } catch (err) {
      setError(err instanceof UserError && err.authRejected ? t("notice.wrongPassword") : err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  async function seen() {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await conn.ackRecoveryNotices();
      onAcked();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login">
      <div className="login-card">
        <div className="login-brand">
          <img src="/favicon.svg" alt="" width={40} height={40} />
          <h1>Warden</h1>
        </div>

        {unseen.length > 0 && (
          <>
            <p className="login-hint">
              <strong>{t("notice.recovered")}</strong>
              {t("notice.recoveredHow")}
            </p>
            <ul>
              {unseen.map((event) => (
                <li key={event.atMs}>
                  {t("notice.event", { when: when(event.atMs), kind: event.kind === "consent" ? t("notice.kindConsent") : t("notice.kindCompany") })}
                </li>
              ))}
            </ul>
            <p className="login-hint">{t("notice.notYou")}</p>
            <button type="button" className="primary-button" disabled={busy} onClick={() => void seen()}>
              {t("notice.understood")}
            </button>
          </>
        )}

        {user.policyPending && (
          <form className="login-card-inner" onSubmit={(e) => void accept(e)}>
            <p className="login-hint">
              <strong>{t("notice.changed")}</strong> {POLICY_TEXT[policy] ? t(POLICY_TEXT[policy]) : ""}
              {t("notice.changedRest")}
            </p>
            <p className="login-hint">{t("notice.reminder")}</p>
            <label>
              {t("notice.passwordToAccept")}
              <input type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} />
            </label>
            {error && <p className="error-banner">{error}</p>}
            <button type="submit" className="primary-button" disabled={busy || !password}>
              {busy ? t("common.wait") : t("notice.accept")}
            </button>
          </form>
        )}

        {error && unseen.length > 0 && !user.policyPending && <p className="error-banner">{error}</p>}
        <button type="button" className="link-button" onClick={onLater}>
          {t("notice.notNow")}
        </button>
      </div>
    </div>
  );
}
