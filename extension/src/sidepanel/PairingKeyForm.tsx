import { useState, type ReactNode } from "react";
import { t } from "../i18n/index.ts";

/** Asks for the hub's pairing key for a change (P120, P123): it is asked for on every change and is not stored anywhere,
 * neither here nor in the background. `error` is the message for a wrong key. */
export default function PairingKeyForm({
  children,
  busy,
  error,
  onConfirm,
  onCancel,
}: {
  children: ReactNode;
  busy: boolean;
  error: string | null;
  onConfirm: (pairingKey: string) => void;
  onCancel: () => void;
}) {
  const [pairingKey, setPairingKey] = useState("");
  return (
    <form
      className="connection-form pairing-key-form"
      onSubmit={(e) => {
        e.preventDefault();
        onConfirm(pairingKey);
      }}
    >
      <p className="skills-hint">{children}</p>
      <label>
        {t("pairing.hubKey")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
      </label>
      {error && <p className="error-banner">{error}</p>}
      <div className="skills-actions">
        <button type="submit" disabled={busy || pairingKey.trim() === ""}>
          {busy ? t("common.wait") : t("common.confirm")}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={onCancel}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );
}
