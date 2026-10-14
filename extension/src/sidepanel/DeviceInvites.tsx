import { useEffect, useState } from "react";
import type { DeviceAdminResponse } from "../background/popup_protocol";
import type { DeviceInviteInfo } from "../protocol/messages";

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
          setError(res.authRejected ? "Chave de pareamento errada." : (res.error ?? "Não foi possível."));
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
      <p className="skills-hint">
        A chave de pareamento só pareia o primeiro aparelho do hub. Para cada outro, crie um convite e entregue o código à pessoa: ela o cola no
        campo da chave ao conectar. O código vale uma vez.
      </p>
      <label>
        Para quem é
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Celular da Ana" />
      </label>
      <label>
        Validade (minutos)
        <input type="number" min={1} max={1440} value={ttlMinutes} onChange={(e) => setTtlMinutes(e.target.value)} />
      </label>
      <label>
        Chave de pareamento do hub
        <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
      </label>
      {error && <p className="error-banner">{error}</p>}
      <div className="skills-actions">
        <button type="submit" disabled={!canInvite}>
          {busy ? "Aguarde…" : "Criar convite"}
        </button>
        {confirmRotate ? (
          <>
            <button type="button" disabled={busy || pairingKey.trim() === ""} onClick={() => run({ type: "rotatePairingKey", pairingKey })}>
              Sim, trocar a chave
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={() => setConfirmRotate(false)}>
              Manter
            </button>
          </>
        ) : (
          <button type="button" className="link-button" disabled={busy || pairingKey.trim() === ""} onClick={() => setConfirmRotate(true)}>
            Trocar a chave de pareamento
          </button>
        )}
      </div>
      {confirmRotate && (
        <p className="skills-hint">
          A chave antiga deixa de abrir a administração do hub. Os aparelhos já pareados continuam funcionando.
        </p>
      )}
      {waiting.length > 0 && (
        <>
          <p className="skills-hint">Convites esperando:</p>
          <ul className="device-invites-waiting">
            {waiting.map((invite) => (
              <li key={invite.name}>
                {invite.name}, até {new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(invite.expiresMs)}{" "}
                <button
                  type="button"
                  className="link-button"
                  disabled={busy || pairingKey.trim() === ""}
                  title={pairingKey.trim() === "" ? "Digite a chave de pareamento acima para cancelar" : undefined}
                  onClick={() => run({ type: "cancelDeviceInvite", pairingKey, name: invite.name })}
                >
                  Cancelar
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {made?.kind === "cancelled" && <p className="skills-hint">Convite de {made.name} cancelado.</p>}
      {made?.kind === "invite" && (
        <p className="skills-hint">
          Convite para {made.name}, válido até {new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" }).format(made.expiresMs)}. O código só
          aparece agora: <code>{made.code}</code>
        </p>
      )}
      {made?.kind === "key" && (
        <p className="skills-hint">
          Chave nova, só aparece agora: <code>{made.newKey}</code>
        </p>
      )}
    </form>
  );
}
