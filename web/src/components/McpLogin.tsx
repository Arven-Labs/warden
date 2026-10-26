import { useCallback, useEffect, useState } from "react";
import { SettingsError, type ServerConnection } from "../hub/connection";
import { checkPastedAddress } from "../hub/mcpLogin";
import { t, type Key } from "../i18n";

/** Signing the hub in to an OAuth MCP server (P131), in the card of that server. The hub does discovery and registration and gives the address
 * to open; the browser ends on an address that does not open (nothing listens there on purpose), and the person copies it back here, where the
 * hub finishes with it. Like any change to the hub's machine it asks for the pairing key. Only for a server that is already saved: the hub
 * reads its address from its own config. */
export default function McpLogin({ conn, savedName }: { conn: ServerConnection | null; savedName: string | undefined }) {
  // `null`: not known yet. `unavailable`: a hub from before the sign-in (or a member), which can only keep the old advice.
  const [state, setState] = useState<"connected" | "disconnected" | "unavailable" | null>(null);
  const [flow, setFlow] = useState<{ flowId: string; authUrl: string; redirectPrefix: string } | null>(null);
  const [asking, setAsking] = useState<"begin" | "forget" | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const load = useCallback(() => {
    if (!conn || !savedName) return;
    conn.listMcpLogins().then(
      (names) => setState(names.includes(savedName) ? "connected" : "disconnected"),
      () => setState("unavailable"),
    );
  }, [conn, savedName]);

  useEffect(load, [load]);

  if (!savedName) return <p className="field-hint settings-field--wide">{t("machine.login.saveFirst")}</p>;
  if (state === "unavailable") return <p className="field-hint settings-field--wide">{t("machine.oauth")}</p>;

  function stopAsking() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  function failed(err: unknown) {
    setMessage({ text: t("machine.login.failed", { error: err instanceof Error ? err.message : String(err) }), error: true });
  }

  async function handleKey() {
    if (!conn || !savedName || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      if (asking === "begin") {
        setFlow(await conn.beginMcpOauth(pairingKey, savedName));
        setPasted("");
        setMessage(null);
      } else {
        const names = await conn.forgetMcpOauth(pairingKey, savedName);
        setState(names.includes(savedName) ? "connected" : "disconnected");
        setMessage({ text: t("machine.login.forgot"), error: false });
      }
      stopAsking();
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        stopAsking();
        failed(err);
      }
    } finally {
      setBusy(false);
    }
  }

  async function handleFinish() {
    if (!conn || !flow) return;
    const checked = checkPastedAddress(pasted, flow.redirectPrefix);
    if ("problem" in checked) {
      setMessage({ text: t(`machine.login.problem.${checked.problem}` as Key), error: true });
      return;
    }
    setBusy(true);
    try {
      await conn.completeMcpOauth(flow.flowId, checked.address);
      setFlow(null);
      setPasted("");
      setState("connected");
      setMessage({ text: t("machine.login.done"), error: false });
    } catch (err) {
      // A wrong address leaves the flow open for the right one; one that ran out of time ends it.
      failed(err);
    } finally {
      setBusy(false);
    }
  }

  const link = flow && /^https?:\/\//.test(flow.authUrl) ? flow.authUrl : null;

  return (
    <div className="settings-field--wide">
      <p className="field-hint">{state === null ? t("machine.login.checking") : state === "connected" ? t("machine.login.connected") : t("machine.login.notConnected")}</p>
      {!flow && !asking && (
        <div className="skills-actions">
          <button type="button" className="link-button" disabled={busy || state === null} onClick={() => setAsking("begin")}>
            {state === "connected" ? t("machine.login.again") : t("machine.login.connect")}
          </button>
          {state === "connected" && (
            <button type="button" className="link-button skills-danger" disabled={busy} onClick={() => setAsking("forget")}>
              {t("machine.login.disconnect")}
            </button>
          )}
        </div>
      )}
      {asking && (
        <div className="skills-editor">
          <span className="skills-hint">{t("machine.login.keyPrompt")}</span>
          <label className="settings-field">
            {t("pairingKey.label")}
            <input
              type="password"
              autoComplete="current-password"
              autoFocus
              value={pairingKey}
              onChange={(e) => setPairingKey(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && pairingKey.trim() !== "" && !busy) {
                  e.preventDefault();
                  void handleKey();
                }
              }}
            />
          </label>
          {keyError && <p className="error-banner">{keyError}</p>}
          <div className="skills-actions">
            <button type="button" className="primary-button" disabled={busy || pairingKey.trim() === ""} onClick={() => void handleKey()}>
              {busy ? t("machine.login.working") : t("common.confirm")}
            </button>
            <button type="button" className="link-button" disabled={busy} onClick={stopAsking}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}
      {flow && (
        <div className="skills-editor">
          <span className="skills-hint">{t("machine.login.steps")}</span>
          {link && (
            <a href={link} target="_blank" rel="noopener noreferrer">
              {t("machine.login.open")}
            </a>
          )}
          <label className="settings-field">
            {t("machine.login.pasteLabel")}
            <input value={pasted} spellCheck={false} autoComplete="off" placeholder={`${flow.redirectPrefix}?code=…`} onChange={(e) => setPasted(e.target.value)} />
          </label>
          <div className="skills-actions">
            <button type="button" className="primary-button" disabled={busy || pasted.trim() === ""} onClick={() => void handleFinish()}>
              {busy ? t("machine.login.working") : t("machine.login.finish")}
            </button>
            <button
              type="button"
              className="link-button"
              disabled={busy}
              onClick={() => {
                setFlow(null);
                setPasted("");
                setMessage(null);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}
      {message && <p className={message.error ? "error-banner" : "field-hint"}>{message.text}</p>}
    </div>
  );
}
