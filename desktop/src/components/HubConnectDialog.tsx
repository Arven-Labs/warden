import { useEffect, useState } from "react";
import type { SavedHub } from "../types";
import type { HubCredential } from "../lib/hub";
import { truthIdProgress, type RemoteState, type TruthIdChallengePayload } from "../lib/hubMap";

interface Props {
  hub: SavedHub;
  /** Signs in with what was typed; rejects with the reason, which is shown here. With a TruthID it resolves at once and the
   * dialog stays: the QR comes as `qr`, and the screen closes it when the hub lets the person in. */
  onSubmit: (credential: HubCredential) => Promise<void>;
  onCancel: () => void;
  /** P113 — where the connection to this hub stands, and the QR the hub made for the TruthID app to scan. */
  state?: RemoteState;
  qr?: TruthIdChallengePayload | null;
}

/** The first sign-in to a hub: its pairing key (the owner), a username and password (a member of its workspace) or a
 * member's TruthID (a QR the phone scans). What is typed is used once and not kept; the hub hands back a token, and that is
 * what this computer remembers. */
function HubConnectDialog({ hub, onSubmit, onCancel, state, qr = null }: Props) {
  const [kind, setKind] = useState<"key" | "member" | "truthid">("key");
  const [key, setKey] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const ready = kind === "key" ? key.trim() !== "" : kind === "member" ? username.trim() !== "" && password !== "" : true;
  const progress = kind === "truthid" && busy ? truthIdProgress(state, qr, now) : null;

  // The QR counts down while it is on the screen.
  useEffect(() => {
    if (kind !== "truthid" || !busy) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [kind, busy]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setError(null);
    try {
      await onSubmit(kind === "key" ? { kind: "key", key } : kind === "member" ? { kind: "member", username, password } : { kind: "truthid" });
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  /** A QR that ran out (or was turned away) is replaced by asking again: a new connection, a new QR. */
  async function askAgain() {
    setError(null);
    setNow(Date.now());
    try {
      await onSubmit({ kind: "truthid" });
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="settings-modal-backdrop" role="dialog" aria-modal="true" aria-labelledby="hub-connect-title">
      <form className="sync-qr-card settings-modal-card approval-card" onSubmit={submit}>
        <h2 id="hub-connect-title" className="approval-title">
          Sign in to {hub.name}
        </h2>
        <p className="settings-hint">
          {hub.url} — what you type is used once and not saved: the hub gives this computer a token, and that is what is remembered.
        </p>
        <div className="hub-connect-kinds" role="radiogroup" aria-label="How to sign in">
          <label>
            <input type="radio" name="hub-kind" checked={kind === "key"} onChange={() => setKind("key")} /> Pairing key (the owner)
          </label>
          <label>
            <input type="radio" name="hub-kind" checked={kind === "member"} onChange={() => setKind("member")} disabled={busy} /> Username and password
          </label>
          <label>
            <input type="radio" name="hub-kind" checked={kind === "truthid"} onChange={() => setKind("truthid")} disabled={busy} /> TruthID (a member)
          </label>
        </div>
        {kind === "key" ? (
          <label className="settings-field">
            <span className="settings-label">Pairing key</span>
            <input className="settings-input" type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.currentTarget.value)} autoFocus />
          </label>
        ) : kind === "truthid" ? (
          <>
            {!busy && <p className="settings-hint">The hub shows a QR; approve it in the TruthID app of the phone your username is linked to. No password is typed here.</p>}
            {busy && qr && (
              <>
                <div className="sync-qr-image" role="img" aria-label="QR for the TruthID app" dangerouslySetInnerHTML={{ __html: qr.svg }} />
                <textarea className="settings-input" readOnly rows={3} value={qr.payload} onFocus={(e) => e.currentTarget.select()} aria-label="The QR as text" />
              </>
            )}
            {busy && !qr && !progress && <p className="settings-hint">Asking the hub for a QR…</p>}
            {progress && <p className={progress.kind === "failed" ? "settings-error-banner" : "settings-hint"}>{progress.text}</p>}
          </>
        ) : (
          <>
            <label className="settings-field">
              <span className="settings-label">Username</span>
              <input className="settings-input" type="text" autoComplete="off" autoCapitalize="none" value={username} onChange={(e) => setUsername(e.currentTarget.value)} autoFocus />
            </label>
            <label className="settings-field">
              <span className="settings-label">Password</span>
              <input className="settings-input" type="password" autoComplete="off" value={password} onChange={(e) => setPassword(e.currentTarget.value)} />
            </label>
          </>
        )}
        {error && <p className="settings-error-banner">{error}</p>}
        <div className="approval-actions">
          <button type="button" className="settings-browse-btn" onClick={onCancel} disabled={busy && kind !== "truthid"}>
            Cancel
          </button>
          {busy && kind === "truthid" ? (
            <button type="button" className="settings-save-btn" onClick={() => void askAgain()}>
              New QR
            </button>
          ) : (
            <button type="submit" className="settings-save-btn" disabled={!ready || busy}>
              {busy ? "Connecting…" : kind === "truthid" ? "Show the QR" : "Connect"}
            </button>
          )}
        </div>
      </form>
    </div>
  );
}

export default HubConnectDialog;
