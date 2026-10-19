import { useEffect, useState } from "react";
import { DeviceError, type ServerConnection } from "../hub/connection";
import type { HubDeviceInvite } from "../hub/messages";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";

// P36 — one-use invites that pair a device without the shared pairing key, and a button that changes that key. Both ask for
// the pairing key each time, like approving or revoking a device. The code and the new key come back once: the hub keeps
// only a hash of the first and the file of the second.

const dateFormatter = { format: (date: Date) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(date) };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

type Made =
  | { kind: "invite"; name: string; code: string; expiresMs: number }
  | { kind: "key"; newKey: string }
  | { kind: "cancelled"; name: string; left: HubDeviceInvite[] };

export default function DeviceInvites({ conn }: { conn: ServerConnection | null }) {
  const [name, setName] = useState("");
  const [ttlMinutes, setTtlMinutes] = useState("15");
  const [pairingKey, setPairingKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [made, setMade] = useState<Made | null>(null);
  const [confirmRotate, setConfirmRotate] = useState(false);
  const [busy, setBusy] = useState(false);
  // The invites that wait, which any paired device may see (by name, never the code).
  const [waiting, setWaiting] = useState<HubDeviceInvite[]>([]);

  useEffect(() => {
    if (!conn) return;
    conn.listDeviceInvites().then(setWaiting).catch((err) => setError(message(err)));
  }, [conn]);

  async function run(action: () => Promise<Made>) {
    setBusy(true);
    setError(null);
    setMade(null);
    try {
      setMade(await action());
      setPairingKey("");
      setConfirmRotate(false);
      if (conn) setWaiting(await conn.listDeviceInvites());
    } catch (err) {
      setError(err instanceof DeviceError && err.authRejected ? t("pairingKey.wrong") : message(err));
    } finally {
      setBusy(false);
    }
  }

  const needsKey = !conn || busy || pairingKey.trim() === "";

  return (
    <section className="devices-invites">
      <h3>{t("invites.title")}</h3>
      <p className="skills-hint">{t("invites.intro")}</p>
      {error && <p className="error-banner">{error}</p>}

      <form
        className="settings-confirm devices-confirm"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => ({ kind: "invite", ...(await conn!.createDeviceInvite(pairingKey, name, Number(ttlMinutes))) }));
        }}
      >
        <label className="settings-field">
          {t("invites.forWhom")}
          <input type="text" placeholder={t("invites.forWhomPlaceholder")} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="settings-field">
          {t("invites.validFor")}
          <input type="text" inputMode="numeric" value={ttlMinutes} onChange={(e) => setTtlMinutes(e.target.value)} />
        </label>
        <label className="settings-field">
          {t("pairingKey.label")}
          <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
          <span className="field-hint">{t("pairingKey.hint")}</span>
        </label>
        <div className="skills-actions">
          <button type="submit" className="primary-button" disabled={needsKey || name.trim() === ""}>
            {busy ? t("common.wait") : t("invites.create")}
          </button>
          {confirmRotate ? (
            <>
              <button
                type="button"
                className="link-button skills-danger"
                disabled={needsKey}
                onClick={() => void run(async () => ({ kind: "key", newKey: await conn!.rotatePairingKey(pairingKey) }))}
              >
                {t("invites.yesRotate")}
              </button>
              <button type="button" className="link-button" disabled={busy} onClick={() => setConfirmRotate(false)}>
                {t("common.keep")}
              </button>
            </>
          ) : (
            <button type="button" className="link-button" disabled={!conn || busy} onClick={() => setConfirmRotate(true)}>
              {t("invites.rotate")}
            </button>
          )}
        </div>
        {confirmRotate && <p className="skills-hint">{t("invites.rotateWarning")}</p>}
      </form>

      {waiting.length > 0 && (
        <>
          <h4>{t("invites.waiting")}</h4>
          <ul className="devices-waiting">
            {waiting.map((invite) => (
              <li key={invite.name}>
                <strong>{invite.name}</strong>
                {t("invites.validUntil", { when: dateFormatter.format(new Date(invite.expiresMs)) })}{" "}
                <button
                  type="button"
                  className="link-button skills-danger"
                  disabled={needsKey}
                  title={needsKey ? t("invites.typeKey") : undefined}
                  onClick={() => void run(async () => ({ kind: "cancelled", name: invite.name, left: await conn!.cancelDeviceInvite(pairingKey, invite.name) }))}
                >
                  {t("common.cancel")}
                </button>
              </li>
            ))}
          </ul>
        </>
      )}

      {made?.kind === "cancelled" && <p className="skills-hint">{t("invites.cancelled", { name: made.name })}</p>}
      {made?.kind === "invite" && (
        <p className="skills-hint">
          {rich("invites.made", {
            name: <strong>{made.name}</strong>,
            when: dateFormatter.format(new Date(made.expiresMs)),
            code: <code>{made.code}</code>,
          })}
        </p>
      )}
      {made?.kind === "key" && <p className="skills-hint">{rich("invites.newKey", { key: <code>{made.newKey}</code> })}</p>}
    </section>
  );
}
