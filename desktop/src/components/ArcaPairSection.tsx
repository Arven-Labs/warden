import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import { emptyView, fieldIsTyped, reduce, type PairView } from "../lib/arcaPair";
import type { SavedHub } from "../types";

type Role = "grant" | "offer";
type Outcome = "done" | "cancelled";

/** P182 item 2, desktop: hands the vault key to a new device (or receives it) through the hub, with the two codes typed by
 * the person. The conversation itself is `warden_sync::arca_pairing_flow`, run by the `arca_pair_run` command. */
function ArcaPairSection() {
  const [hubs, setHubs] = useState<SavedHub[] | null>(null);
  const [hubId, setHubId] = useState("");
  const [role, setRole] = useState<Role | null>(null);
  const [running, setRunning] = useState(false);
  const [view, setView] = useState<PairView>(emptyView);
  const [typed, setTyped] = useState("");
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const runningRef = useRef(false);

  useEffect(() => {
    invoke<SavedHub[]>("list_hubs")
      .then((list) => {
        setHubs(list);
        setHubId((current) => current || list[0]?.id || "");
      })
      .catch(() => setHubs([]));
    invoke<Role>("arca_pair_role").then(setRole).catch(() => setRole(null));
  }, []);

  useEffect(() => {
    const unlisten = listen<unknown>("arca-pair-event", (event) => setView((current) => reduce(current, event.payload)));
    return () => {
      unlisten.then((off) => off());
      // Leaving the screen is giving up: a pairing nobody can see must not keep waiting for a grant.
      if (runningRef.current) void invoke("arca_pair_cancel");
    };
  }, []);

  async function start() {
    setResult(null);
    setView(emptyView);
    setTyped("");
    setRunning(true);
    runningRef.current = true;
    const playedAs = role;
    try {
      const outcome = await invoke<Outcome>("arca_pair_run", { hubId });
      setResult(
        outcome === "cancelled"
          ? { ok: false, text: t("arcaPair.cancelled") }
          : { ok: true, text: t(playedAs === "offer" ? "arcaPair.doneOffer" : "arcaPair.doneGrant") },
      );
    } catch (e) {
      setResult({ ok: false, text: t("arcaPair.failed", { message: String(e) }) });
    } finally {
      runningRef.current = false;
      setRunning(false);
      setView(emptyView);
      invoke<Role>("arca_pair_role").then(setRole).catch(() => {});
    }
  }

  function answer(reply: object) {
    void invoke("arca_pair_answer", { reply }).catch((e) => setResult({ ok: false, text: t("arcaPair.failed", { message: String(e) }) }));
  }

  function submit(e: React.FormEvent) {
    e.preventDefault();
    if (typed.trim().length === 0) return;
    answer({ kind: "typed", text: typed.trim() });
    setTyped("");
  }

  const noHubs = hubs !== null && hubs.length === 0;
  const fieldLabel = view.question === "code" ? t("arcaPair.askCode") : t("arcaPair.askFingerprint");

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("arcaPair.title")}</h3>
      </div>
      <div className="sync-pairing-block">
        {role && <p className="settings-hint">{t(role === "grant" ? "arcaPair.hintGrant" : "arcaPair.hintOffer")}</p>}

        {noHubs ? (
          <p className="settings-hint">{t("arcaPair.noHubs")}</p>
        ) : (
          !running && (
            <div className="sync-join-form">
              <label className="settings-hint" htmlFor="arca-pair-hub">
                {t("arcaPair.hub")}
              </label>
              <select id="arca-pair-hub" className="settings-input" value={hubId} onChange={(e) => setHubId(e.target.value)}>
                {(hubs ?? []).map((hub) => (
                  <option key={hub.id} value={hub.id}>
                    {hub.name}
                  </option>
                ))}
              </select>
              <button type="button" className="settings-save-btn" disabled={hubId === "" || role === null} onClick={() => void start()}>
                {t("arcaPair.start")}
              </button>
            </div>
          )
        )}

        {running && (
          <>
            {view.shown && (
              <>
                <p className="settings-hint">{t(view.shown.what === "offerCode" ? "arcaPair.offerCodeHint" : "arcaPair.fingerprintHint")}</p>
                <p className="sync-pairing-code">{view.shown.value}</p>
              </>
            )}

            {view.question === null && !view.shown && <p className="settings-hint">…</p>}
            {view.shown?.what === "offerCode" && <p className="settings-hint">{t("arcaPair.waiting")}</p>}

            {view.question === "choice" && (
              <>
                <p className="settings-hint">{t("arcaPair.choose")}</p>
                <ul>
                  {view.devices.map((device, index) => (
                    <li key={device}>
                      <button type="button" className="settings-browse-btn" onClick={() => answer({ kind: "picked", index })}>
                        {device}
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            )}

            {(view.question === "code" || view.question === "fingerprint") && (
              <form className="sync-join-form" onSubmit={submit}>
                <label className="settings-hint" htmlFor="arca-pair-typed">
                  {fieldLabel}
                </label>
                <input
                  key={view.question}
                  id="arca-pair-typed"
                  type="text"
                  className="settings-input"
                  value={typed}
                  autoComplete="off"
                  autoCorrect="off"
                  autoCapitalize="off"
                  spellCheck={false}
                  autoFocus
                  onChange={(e) => setTyped(e.target.value)}
                  onBeforeInput={(e) => {
                    if (!fieldIsTyped(e.nativeEvent as InputEvent)) e.preventDefault();
                  }}
                  onPaste={(e) => e.preventDefault()}
                  onDrop={(e) => e.preventDefault()}
                />
                <button type="submit" className="settings-save-btn" disabled={typed.trim().length === 0}>
                  {t("arcaPair.confirm")}
                </button>
                <p className="settings-hint">{t("arcaPair.typeHint")}</p>
              </form>
            )}

            <button type="button" className="settings-browse-btn" onClick={() => void invoke("arca_pair_cancel")}>
              {t("arcaPair.cancel")}
            </button>
          </>
        )}

        {result && <p className={result.ok ? "settings-success-banner" : "settings-error-banner"}>{result.text}</p>}
      </div>
    </section>
  );
}

export default ArcaPairSection;
