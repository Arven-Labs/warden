import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { t, type Key } from "../i18n";
import { hubBeginMcpOauth, hubCompleteMcpOauth, hubForgetMcpOauth, hubListMcpLogins, hubOauthServers } from "../lib/hub";
import { callbackAddress } from "../lib/hubMcpLogin";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** "Hub sign-ins" (P131): the hub in use signs in to its OAuth MCP servers (Slack, Notion, Sentry...) and keeps the login on its own
 * machine; this computer only catches the browser's return. It opens a listener on `127.0.0.1`, tells the hub its port, opens the page the
 * hub answers with, and hands the hub what arrives, so nothing is copied. Which servers the hub has is edited on its web page: only the
 * saved ones that sign in with OAuth are listed. The owner's only (the pairing key asks at each sign-in and sign-out). */
function HubMcpLogins() {
  const { askKey, dialog } = usePairingKey();
  const [servers, setServers] = useState<string[] | null>(null);
  const [logins, setLogins] = useState<string[]>([]);
  const [working, setWorking] = useState<{ name: string; waiting: boolean } | null>(null);
  const [message, setMessage] = useState<{ text: string; error: boolean } | null>(null);

  const load = useCallback(async () => {
    try {
      const [names, signedIn] = await Promise.all([hubOauthServers(), hubListMcpLogins()]);
      setServers(names);
      setLogins(signedIn);
    } catch (err) {
      // A member, or a hub from before the sign-in: nothing to show, and no noise about it.
      setServers(null);
      setMessage({ text: t("st.hubLogin.loadFailed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function handleConnect(name: string) {
    setWorking({ name, waiting: false });
    setMessage(null);
    try {
      const port = await invoke<number>("mcp_login_listen");
      const started = await hubBeginMcpOauth(askKey, name, port);
      if (/^https?:\/\//.test(started.authUrl)) await openUrl(started.authUrl);
      setWorking({ name, waiting: true });
      const query = await invoke<string>("mcp_login_wait");
      const returned = callbackAddress(started.redirectPrefix, query);
      if ("problem" in returned) {
        setMessage({ text: t(`st.hubLogin.problem.${returned.problem}` as Key), error: true });
        return;
      }
      await hubCompleteMcpOauth(started.flowId, returned.address);
      setMessage({ text: t("st.hubLogin.done"), error: false });
      await load();
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setMessage({ text: t("st.hubLogin.failed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    } finally {
      await invoke("mcp_login_cancel").catch(() => undefined);
      setWorking(null);
    }
  }

  async function handleDisconnect(name: string) {
    setWorking({ name, waiting: false });
    setMessage(null);
    try {
      setLogins(await hubForgetMcpOauth(askKey, name));
      setMessage({ text: t("st.hubLogin.forgot"), error: false });
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setMessage({ text: t("st.hubLogin.failed", { error: err instanceof Error ? err.message : String(err) }), error: true });
    } finally {
      setWorking(null);
    }
  }

  // A member, or a hub that does not offer the sign-in, has nothing here.
  if (servers === null && message === null) return null;

  return (
    <section className="settings-section">
      {dialog}
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("st.hubLogin.title")}</h3>
      </div>
      <p className="settings-hint">{t("st.hubLogin.hint")}</p>
      {servers !== null && servers.length === 0 && <p className="settings-hint">{t("st.hubLogin.none")}</p>}
      {servers !== null && servers.length > 0 && (
        <div className="provider-list">
          {servers.map((name) => {
            const signedIn = logins.includes(name);
            const mine = working?.name === name;
            return (
              <div className="provider-card skill-card" key={name}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{name}</span>
                  <div className="skill-card-actions">
                    <span className="settings-hint">{signedIn ? t("st.hubLogin.connected") : t("st.hubLogin.notConnected")}</span>
                    {mine && working.waiting ? (
                      <button type="button" className="settings-browse-btn" onClick={() => void invoke("mcp_login_cancel")}>
                        {t("common.cancel")}
                      </button>
                    ) : (
                      <button type="button" className="settings-browse-btn" onClick={() => void handleConnect(name)} disabled={working !== null}>
                        {signedIn ? t("st.hubLogin.again") : t("st.hubLogin.connect")}
                      </button>
                    )}
                    {signedIn && (
                      <button type="button" className="provider-delete-btn" onClick={() => void handleDisconnect(name)} disabled={working !== null}>
                        {t("st.hubLogin.disconnect")}
                      </button>
                    )}
                  </div>
                </div>
                {mine && working.waiting && <p className="settings-hint">{t("st.hubLogin.waiting")}</p>}
              </div>
            );
          })}
        </div>
      )}
      {message && <p className={message.error ? "settings-error-banner" : "settings-hint"}>{message.text}</p>}
    </section>
  );
}

export default HubMcpLogins;
