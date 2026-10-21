import { useState } from "react";
import { UserError, type ServerConnection } from "../hub/connection";
import type { UserInfo } from "../hub/messages";
import { intlTag, t, type Key } from "../i18n";

// Recovery (P84 slice 4 parte B): who besides a person may open their encrypted data. `private`: nobody.
// `consent`: the owner's recovery key and the person's recovery code, together. `company`: the owner
// alone, recorded and told to the person. The owner's key is shown once, when it's made, and typed in
// for each recovery — the hub only keeps its public half. Every change asks for the pairing key.

const POLICIES: Array<{ id: string; name: Key; text: Key }> = [
  { id: "private", name: "recoveryPolicy.private", text: "recoveryPolicy.privateText" },
  { id: "consent", name: "recoveryPolicy.consent", text: "recoveryPolicy.consentText" },
  { id: "company", name: "recoveryPolicy.company", text: "recoveryPolicy.companyText" },
];

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function when(atMs: number): string {
  return new Date(atMs).toLocaleString(intlTag());
}

interface Props {
  conn: ServerConnection | null;
  users: UserInfo[];
  /** The workspace's policy now. */
  policy: string;
  /** The lists changed (a policy set, a member recovered). `users` is `null` when only the policy changed. */
  onChanged: (users: UserInfo[] | null, policy: string, tempPassword?: { id: string; password: string }) => void;
}

export default function RecoveryPolicySection({ conn, users, policy, onChanged }: Props) {
  const [chosen, setChosen] = useState(policy);
  const [newKey, setNewKey] = useState(false);
  const [pairingKey, setPairingKey] = useState("");
  const [recovering, setRecovering] = useState<{ user: UserInfo; recoveryKey: string; code: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The owner's recovery key, shown once. */
  const [secret, setSecret] = useState<string | null>(null);

  const encrypted = users.filter((u) => u.encrypted);

  async function apply(e: React.FormEvent) {
    e.preventDefault();
    if (!conn || busy) return;
    setBusy(true);
    setError(null);
    try {
      const reply = await conn.setRecoveryPolicy(pairingKey, chosen, newKey);
      setSecret(reply.secret ?? null);
      setNewKey(false);
      setPairingKey("");
      onChanged(null, reply.policy);
    } catch (err) {
      setError(err instanceof UserError && err.authRejected ? t("pairingKey.wrong") : message(err));
    } finally {
      setBusy(false);
    }
  }

  async function recover(e: React.FormEvent) {
    e.preventDefault();
    if (!conn || !recovering || busy) return;
    setBusy(true);
    setError(null);
    try {
      const reply = await conn.recoverMember(pairingKey, recovering.user.id, recovering.recoveryKey, recovering.code.trim() || undefined);
      setPairingKey("");
      const id = recovering.user.id;
      setRecovering(null);
      onChanged(reply.users, reply.recoveryPolicy ?? policy, reply.tempPassword ? { id, password: reply.tempPassword } : undefined);
    } catch (err) {
      setError(err instanceof UserError && err.authRejected ? t("pairingKey.wrong") : message(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="skills-header">
        <h2>{t("recoveryPolicy.title")}</h2>
      </div>
      <p className="skills-hint">{t("recoveryPolicy.intro")}</p>

      {secret && (
        <div className="error-banner">
          <p>
            <strong>{t("recoveryPolicy.yourKey")}</strong>
            {t("recoveryPolicy.yourKeyRest")}
          </p>
          <p className="recovery-code">
            <code>{secret}</code>
          </p>
          <button type="button" className="link-button" onClick={() => setSecret(null)}>
            {t("recoveryPolicy.hide")}
          </button>
        </div>
      )}

      <form className="settings-confirm devices-confirm" onSubmit={(e) => void apply(e)}>
        <fieldset className="settings-field">
          <legend>{t("recoveryPolicy.workspacePolicy")}</legend>
          {POLICIES.map((p) => (
            <label key={p.id} className="checkbox-row">
              <input type="radio" name="recovery-policy" checked={chosen === p.id} onChange={() => setChosen(p.id)} />
              <span>
                <strong>{t(p.name)}</strong> — {t(p.text)}
              </span>
            </label>
          ))}
          {chosen !== policy && chosen !== "private" && <span className="field-hint">{t("recoveryPolicy.changeNote")}</span>}
        </fieldset>
        {chosen !== "private" && (
          <label className="checkbox-row">
            <input type="checkbox" checked={newKey} onChange={(e) => setNewKey(e.target.checked)} />
            {t("recoveryPolicy.newKey")}
          </label>
        )}
        <label className="settings-field">
          {t("pairingKey.label")}
          <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        </label>
        {error && !recovering && <p className="error-banner">{error}</p>}
        <div className="skills-actions">
          <button type="submit" className="primary-button" disabled={busy || !conn || pairingKey.trim() === "" || (chosen === policy && !newKey)}>
            {busy ? t("common.wait") : t("recoveryPolicy.apply")}
          </button>
        </div>
      </form>

      {policy !== "private" && encrypted.length > 0 && (
        <>
          <h3>{t("recoveryPolicy.recoverTitle")}</h3>
          <ul className="skills-list">
            {encrypted.map((user) => {
              const mine = recovering?.user.id === user.id ? recovering : null;
              const can = user.memberPolicy === "consent" || user.memberPolicy === "company";
              return (
                <li key={user.id} className="skills-item">
                  <div className="skills-item-header">
                    <span className="skills-item-name">{user.name}</span>
                    <span className="devices-status devices-status--approved">{user.policyPending ? t("recoveryPolicy.pending") : (user.memberPolicy ?? "")}</span>
                  </div>
                  <p className="skills-item-description">
                    {can ? (user.memberPolicy === "consent" ? t("recoveryPolicy.needsBoth") : t("recoveryPolicy.needsKey")) : t("recoveryPolicy.stillPrivate")}
                    {user.recoveries &&
                      user.recoveries.length > 0 &&
                      t("recoveryPolicy.recovered", { count: user.recoveries.length, when: when(user.recoveries[user.recoveries.length - 1].atMs) })}
                  </p>
                  {mine ? (
                    <form className="settings-confirm devices-confirm" onSubmit={(e) => void recover(e)}>
                      <label className="settings-field">
                        {t("recoveryPolicy.yourRecoveryKey")}
                        <input type="text" autoComplete="off" spellCheck={false} value={mine.recoveryKey} onChange={(e) => setRecovering({ ...mine, recoveryKey: e.target.value })} />
                      </label>
                      {user.memberPolicy === "consent" && (
                        <label className="settings-field">
                          {t("recoveryPolicy.personCode")}
                          <input type="text" autoComplete="off" spellCheck={false} value={mine.code} onChange={(e) => setRecovering({ ...mine, code: e.target.value })} />
                        </label>
                      )}
                      <label className="settings-field">
                        {t("pairingKey.label")}
                        <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
                      </label>
                      <p className="field-hint">{t("recoveryPolicy.aftermath")}</p>
                      {error && <p className="error-banner">{error}</p>}
                      <div className="skills-actions">
                        <button
                          type="submit"
                          className="primary-button"
                          disabled={busy || pairingKey.trim() === "" || mine.recoveryKey.trim() === "" || (user.memberPolicy === "consent" && mine.code.trim() === "")}
                        >
                          {busy ? t("common.wait") : t("recoveryPolicy.recover")}
                        </button>
                        <button type="button" className="link-button" disabled={busy} onClick={() => setRecovering(null)}>
                          {t("common.cancel")}
                        </button>
                      </div>
                    </form>
                  ) : (
                    <div className="skills-actions">
                      <button
                        type="button"
                        className="link-button"
                        disabled={!conn || !can || recovering !== null}
                        onClick={() => {
                          setError(null);
                          setRecovering({ user, recoveryKey: "", code: "" });
                        }}
                      >
                        {t("recoveryPolicy.recover")}
                      </button>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </>
  );
}
