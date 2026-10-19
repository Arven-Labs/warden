import { useEffect, useState } from "react";
import type { DeviceAdminResponse } from "../background/popup_protocol";
import type { DeviceInviteInfo } from "../protocol/messages";
import { intlTag, t } from "../i18n/index.ts";
import { rich } from "../i18n/react.tsx";

const whenLabel = (ms: number) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(ms);

type Made = { kind: "invite"; name: string; code: string; expiresMs: number } | { kind: "key"; newKey: string } | { kind: "cancelled"; name: string };

type Request =
  | { type: "createDeviceInvite"; pairingKey: string; name: string; ttlMinutes: number }
  | { type: "rotatePairingKey"; pairingKey: string }
  | { type: "cancelDeviceInvite"; pairingKey: string; name: string };

/** Aparelhos (P36): cria um convite de uso único para parear mais um aparelho e troca a chave de pareamento do hub. A chave de pareamento
 * só pareia o primeiro aparelho; os outros entram com o código do convite. A chave é pedida a cada ação e não fica guardada em lugar nenhum;
 * o código do convite (e a chave nova) aparecem uma vez só, porque o hub guarda apenas o hash do código. */
export default function DeviceInvites() {
  const [name, setName] = useState("");
  const [ttlMinutes, setTtlMinutes] = useState("15");
  const [pairingKey, setPairingKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [made, setMade] = useState<Made | null>(null);
  const [confirmRotate, setConfirmRotate] = useState(false);
  const [busy, setBusy] = useState(false);
  // Os convites que esperam, que qualquer aparelho pareado pode ver (pelo nome, nunca o código).
  const [waiting, setWaiting] = useState<DeviceInviteInfo[]>([]);

  function refreshWaiting() {
    chrome.runtime.sendMessage({ type: "listDeviceInvites" }).then((res: DeviceAdminResponse) => {
      if (res.ok && res.invites) setWaiting(res.invites);
    });
  }

  useEffect(refreshWaiting, []);

  function run(request: Request) {
    setBusy(true);
    setError(null);
    setMade(null);
    chrome.runtime
      .sendMessage(request)
      .then((res: DeviceAdminResponse) => {
        if (!res.ok) {
          setError(res.authRejected ? t("pairing.wrong") : (res.error ?? t("invites.cantDo")));
          return;
        }
        if (request.type === "cancelDeviceInvite") {
          setMade({ kind: "cancelled", name: request.name });
          setWaiting(res.invites ?? []);
        } else if (res.invite) {
          setMade({ kind: "invite", ...res.invite });
          refreshWaiting();
        } else if (res.newKey) {
          setMade({ kind: "key", newKey: res.newKey });
        }
        setPairingKey("");
        setConfirmRotate(false);
      })
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
  }

  const minutes = Number(ttlMinutes);
  const canInvite = !busy && pairingKey.trim() !== "" && name.trim() !== "" && Number.isFinite(minutes) && minutes >= 1;

  return (
    <form
      className="connection-form device-invites"
      onSubmit={(e) => {
        e.preventDefault();
        if (canInvite) run({ type: "createDeviceInvite", pairingKey, name: name.trim(), ttlMinutes: Math.floor(minutes) });
      }}
    >
      <p className="skills-hint">{t("invites.intro")}</p>
      <label>
        {t("invites.forWhom")}
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("invites.forWhomPlaceholder")} />
      </label>
      <label>
        {t("invites.validity")}
        <input type="number" min={1} max={1440} value={ttlMinutes} onChange={(e) => setTtlMinutes(e.target.value)} />
      </label>
      <label>
        {t("pairing.hubKey")}
        <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
      </label>
      {error && <p className="error-banner">{error}</p>}
      <div className="skills-actions">
        <button type="submit" disabled={!canInvite}>
          {busy ? t("common.wait") : t("invites.create")}
        </button>
        {confirmRotate ? (
          <>
            <button type="button" disabled={busy || pairingKey.trim() === ""} onClick={() => run({ type: "rotatePairingKey", pairingKey })}>
              {t("invites.yesRotate")}
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={() => setConfirmRotate(false)}>
              {t("common.keep")}
            </button>
          </>
        ) : (
          <button type="button" className="link-button" disabled={busy || pairingKey.trim() === ""} onClick={() => setConfirmRotate(true)}>
            {t("invites.rotate")}
          </button>
        )}
      </div>
      {confirmRotate && <p className="skills-hint">{t("invites.rotateWarning")}</p>}
      {waiting.length > 0 && (
        <>
          <p className="skills-hint">{t("invites.waiting")}</p>
          <ul className="device-invites-waiting">
            {waiting.map((invite) => (
              <li key={invite.name}>
                {t("invites.until", { name: invite.name, when: whenLabel(invite.expiresMs) })}{" "}
                <button
                  type="button"
                  className="link-button"
                  disabled={busy || pairingKey.trim() === ""}
                  title={pairingKey.trim() === "" ? t("invites.typeKey") : undefined}
                  onClick={() => run({ type: "cancelDeviceInvite", pairingKey, name: invite.name })}
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
        <p className="skills-hint">{rich("invites.made", { name: made.name, when: whenLabel(made.expiresMs), code: <code>{made.code}</code> })}</p>
      )}
      {made?.kind === "key" && <p className="skills-hint">{rich("invites.newKey", { key: <code>{made.newKey}</code> })}</p>}
    </form>
  );
}
