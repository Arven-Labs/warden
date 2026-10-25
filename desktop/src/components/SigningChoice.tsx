import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { t } from "../i18n";
import { rememberKeyPath, shortSigner } from "../lib/signingKey";
import CopyButton from "./CopyButton";

/** What the person decided about signing a skill they share (P160): whether to, and with which key file on this computer. */
export interface SigningState {
  sign: boolean;
  keyPath: string | null;
}

/** The "Sign it with my key" part of the export and publish forms. The key file stays on this computer: the app reads it for the one
 * signature (`sign_skill_package`) and only its path is remembered, so the next share comes with the same key already chosen. */
function SigningChoice({ value, onChange }: { value: SigningState; onChange: (next: SigningState) => void }) {
  const [signer, setSigner] = useState<string | null>(null);
  const [madeKey, setMadeKey] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  // Whose key the chosen file is, so the person sees whose signature it will be; a remembered file that moved or broke is forgotten.
  useEffect(() => {
    if (!value.keyPath) {
      setSigner(null);
      return;
    }
    let current = true;
    invoke<string>("signing_key_public", { path: value.keyPath }).then(
      (key) => {
        if (current) {
          setSigner(key);
          setProblem(null);
        }
      },
      (err) => {
        if (!current) return;
        setSigner(null);
        setProblem(t("skills.signFailed", { error: String(err) }));
        rememberKeyPath(null);
        onChange({ sign: value.sign, keyPath: null });
      },
    );
    return () => {
      current = false;
    };
    // `onChange` is a new function on every render of the caller; the file is what this reads again for.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value.keyPath]);

  function choose(path: string) {
    rememberKeyPath(path);
    setMadeKey(null);
    onChange({ sign: true, keyPath: path });
  }

  async function handleChoose() {
    setProblem(null);
    try {
      const path = await open({ multiple: false });
      if (typeof path === "string") choose(path);
    } catch (err) {
      setProblem(t("skills.signFailed", { error: String(err) }));
    }
  }

  async function handleCreate() {
    setProblem(null);
    try {
      const path = await save({ defaultPath: "warden-signing.key" });
      if (!path) return;
      const publicKey = await invoke<string>("create_signing_key", { path });
      choose(path);
      setMadeKey(publicKey);
    } catch (err) {
      setProblem(t("skills.signFailed", { error: String(err) }));
    }
  }

  return (
    <>
      <label className="skill-agent-option">
        <input type="checkbox" checked={value.sign} onChange={(e) => onChange({ sign: e.currentTarget.checked, keyPath: value.keyPath })} />
        {t("skills.signWith")}
      </label>
      {value.sign && (
        <>
          <p className="settings-hint">{t("skills.signHint")}</p>
          {signer ? <p className="settings-hint">{t("skills.signAs", { key: shortSigner(signer) })}</p> : <p className="settings-hint">{t("skills.signNeedsKey")}</p>}
          <div className="skill-editor-actions">
            <button type="button" className="settings-browse-btn" onClick={() => void handleChoose()}>
              {t("skills.signChooseKey")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => void handleCreate()}>
              {t("skills.signCreateKey")}
            </button>
          </div>
          {madeKey && (
            <>
              <p className="settings-hint">{t("skills.signKeyMade")}</p>
              <code>{madeKey}</code>
              <CopyButton text={madeKey} className="settings-browse-btn" label={t("skills.signCopyPublic")} />
            </>
          )}
        </>
      )}
      {problem && <p className="settings-error-banner">{problem}</p>}
    </>
  );
}

export default SigningChoice;
