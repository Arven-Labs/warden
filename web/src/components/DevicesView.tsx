import { useCallback, useEffect, useState } from "react";
import { DeviceError, type DeviceList, type ServerConnection } from "../hub/connection";
import type { HubDevice } from "../hub/messages";
import DeviceInvites from "./DeviceInvites";
import NodesSection from "./NodesSection";
import { intlTag, t } from "../i18n";

// The hub's paired devices (Session 103) — the same list, approve and revoke as the desktop's
// Workspace screen and `warden-server devices`, so a hub with no screen can be managed from here.
// Listing is open to any logged-in browser; approving or revoking asks for the pairing key each
// time, like saving settings. Invites and changing the pairing key (P36) are in `DeviceInvites`.

type Action = "approve" | "revoke";

const STATUS_KEY = { pending: "devices.status.pending", approved: "devices.status.approved", revoked: "devices.status.revoked" } as const;

const dateFormatter = { format: (date: Date) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(date) };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function DevicesView({ conn }: { conn: ServerConnection | null }) {
  const [list, setList] = useState<DeviceList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<{ device: HubDevice; action: Action } | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** Bumped when the device list changes, so the nodes below reread their "approved" state. */
  const [refreshKey, setRefreshKey] = useState(0);

  const load = useCallback(async () => {
    if (!conn) return;
    setError(null);
    try {
      setList(await conn.listDevices());
      setRefreshKey((k) => k + 1);
    } catch (err) {
      setError(message(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
  }, [load]);

  function cancel() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  async function confirm() {
    if (!conn || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      setList(await conn.setDeviceStatus(pairingKey, asking.device.deviceId, asking.action));
      setRefreshKey((k) => k + 1);
      cancel();
    } catch (err) {
      if (err instanceof DeviceError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancel();
        setError(message(err));
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="usage-view devices-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("devices.intro")}</span>
        <button type="button" className="link-button" onClick={() => void load()} disabled={!conn || busy}>
          {t("devices.reload")}
        </button>
      </div>

      {error && <p className="error-banner">{error}</p>}
      {!list && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {list && list.devices.length === 0 && <p className="skills-hint">{t("devices.none")}</p>}

      {list && list.devices.length > 0 && (
        <ul className="skills-list">
          {list.devices.map((device) => {
            const isYou = device.deviceId === list.you;
            const isAsking = asking?.device.deviceId === device.deviceId;
            return (
              <li key={device.deviceId} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">
                    {device.deviceName}
                    {isYou && <span className="devices-you">{t("devices.thisBrowser")}</span>}
                  </span>
                  <span className={`devices-status devices-status--${device.status}`}>{t(STATUS_KEY[device.status])}</span>
                </div>
                <p className="skills-item-description">
                  <code>{device.deviceId}</code>
                  {device.user && t("devices.from", { user: device.user })}
                  {t("devices.lastSeen", { when: dateFormatter.format(new Date(device.lastSeenMs)) })}
                </p>

                {isAsking ? (
                  <form
                    className="settings-confirm devices-confirm"
                    onSubmit={(e) => {
                      e.preventDefault();
                      void confirm();
                    }}
                  >
                    {asking.action === "revoke" && isYou && <p className="error-banner">{t("devices.revokeSelf")}</p>}
                    <label className="settings-field">
                      {t("pairingKey.label")}
                      <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
                      <span className="field-hint">{t("pairingKey.hint")}</span>
                    </label>
                    {keyError && <p className="error-banner">{keyError}</p>}
                    <div className="skills-actions">
                      <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
                        {busy ? t("common.wait") : asking.action === "approve" ? t("devices.approve") : t("devices.revoke")}
                      </button>
                      <button type="button" className="link-button" disabled={busy} onClick={cancel}>
                        {t("common.cancel")}
                      </button>
                    </div>
                  </form>
                ) : (
                  <div className="skills-actions">
                    {device.status !== "approved" && (
                      <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ device, action: "approve" })}>
                        {device.status === "revoked" ? t("devices.approveAgain") : t("devices.approve")}
                      </button>
                    )}
                    {device.status !== "revoked" && (
                      <button
                        type="button"
                        className="link-button skills-danger"
                        disabled={!conn || asking !== null}
                        onClick={() => setAsking({ device, action: "revoke" })}
                      >
                        {t("devices.revoke")}
                      </button>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <DeviceInvites conn={conn} />

      <NodesSection conn={conn} refreshKey={refreshKey} />
    </div>
  );
}
