import { useState } from "react";
import { DeviceError, type ServerConnection } from "../hub/connection";

// P36 — one-use invites that pair a device without the shared pairing key, and a button that changes that key. Both ask for
// the pairing key each time, like approving or revoking a device. The code and the new key come back once: the hub keeps
// only a hash of the first and the file of the second.

const dateFormatter = new Intl.DateTimeFormat("pt-BR", { dateStyle: "short", timeStyle: "short" });

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

type Made = { kind: "invite"; name: string; code: string; expiresMs: number } | { kind: "key"; newKey: string };

export default function DeviceInvites({ conn }: { conn: ServerConnection | null }) {
  const [name, setName] = useState("");
  const [ttlMinutes, setTtlMinutes] = useState("15");
  const [pairingKey, setPairingKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [made, setMade] = useState<Made | null>(null);
  const [confirmRotate, setConfirmRotate] = useState(false);
  const [busy, setBusy] = useState(false);

  async function run(action: () => Promise<Made>) {
    setBusy(true);
    setError(null);
    setMade(null);
    try {
      setMade(await action());
      setPairingKey("");
      setConfirmRotate(false);
    } catch (err) {
      setError(err instanceof DeviceError && err.authRejected ? "Chave de pareamento errada." : message(err));
    } finally {
      setBusy(false);
    }
  }

  const needsKey = !conn || busy || pairingKey.trim() === "";

  return (
    <section className="devices-invites">
      <h3>Convites e chave de pareamento</h3>
      <p className="skills-hint">
        Um convite pareia um aparelho uma vez e expira: ninguém precisa da chave compartilhada, e um aparelho revogado não volta
        pareando de novo com outro id. O código aparece só uma vez.
      </p>
      {error && <p className="error-banner">{error}</p>}

      <form
        className="settings-confirm devices-confirm"
        onSubmit={(e) => {
          e.preventDefault();
          void run(async () => ({ kind: "invite", ...(await conn!.createDeviceInvite(pairingKey, name, Number(ttlMinutes))) }));
        }}
      >
        <label className="settings-field">
          Para quem é
          <input type="text" placeholder="Celular da Ana" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="settings-field">
          Vale por (minutos)
          <input type="text" inputMode="numeric" value={ttlMinutes} onChange={(e) => setTtlMinutes(e.target.value)} />
        </label>
        <label className="settings-field">
          Chave de pareamento do hub
          <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
          <span className="field-hint">A mesma do primeiro login. É pedida a cada mudança.</span>
        </label>
        <div className="skills-actions">
          <button type="submit" className="primary-button" disabled={needsKey || name.trim() === ""}>
            {busy ? "Aguarde…" : "Criar convite"}
          </button>
          {confirmRotate ? (
            <>
              <button
                type="button"
                className="link-button skills-danger"
                disabled={needsKey}
                onClick={() => void run(async () => ({ kind: "key", newKey: await conn!.rotatePairingKey(pairingKey) }))}
              >
                Sim, trocar a chave
              </button>
              <button type="button" className="link-button" disabled={busy} onClick={() => setConfirmRotate(false)}>
                Manter
              </button>
            </>
          ) : (
            <button type="button" className="link-button" disabled={!conn || busy} onClick={() => setConfirmRotate(true)}>
              Trocar a chave de pareamento
            </button>
          )}
        </div>
        {confirmRotate && (
          <p className="skills-hint">
            A chave velha deixa de parear aparelhos novos e de abrir a administração do hub. Aparelhos já pareados continuam
            funcionando. Digite a chave atual acima e confirme.
          </p>
        )}
      </form>

      {made?.kind === "invite" && (
        <p className="skills-hint">
          Convite para <strong>{made.name}</strong> (vale até {dateFormatter.format(new Date(made.expiresMs))}): <code>{made.code}</code>
        </p>
      )}
      {made?.kind === "key" && (
        <p className="skills-hint">
          Nova chave de pareamento (só aparece agora): <code>{made.newKey}</code>. Use esta nas próximas mudanças.
        </p>
      )}
    </section>
  );
}
