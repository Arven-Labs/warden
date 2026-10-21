import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";
import { ApiKeyField } from "./SettingsView";
import HubsSection from "./HubsSection";
import type { DiscoveredHub, EmbeddedServerConfig, EmbeddedServerStatus, HubPairingConfig, HubPairingQr, PairedDevice } from "../types";

const STOPPED_STATUS: EmbeddedServerStatus = { running: false, boundAddr: null, serverName: null, secure: false, secureUrl: null, webUrl: null };

function formatSeen(ms: number): string {
  return new Intl.DateTimeFormat(intlTag(), { dateStyle: "medium", timeStyle: "short" }).format(new Date(ms));
}

function StatusBadge({ status }: { status: PairedDevice["status"] }) {
  const label = status === "pending" ? t("ws.pending") : status === "approved" ? t("ws.approved") : t("ws.revoked");
  return <span className={`storage-provider-badge workspace-status-badge workspace-status-badge--${status}`}>{label}</span>;
}

type HttpsMode = "none" | "tailscale" | "own";

function newConfig(authKey: string): EmbeddedServerConfig {
  return { port: 7420, listenHost: null, authKey, serverName: null, tailscaleCert: false, tlsCert: null, tlsKey: null, tlsHost: null, webUi: true };
}

/** Phase 9.1 follow-up ("virar o hub desta rede") — lets this same desktop app embed its own
 * `warden-server` instead of that always being a separate process. Every `warden-server serve`
 * flag has a field here (Session 103), so nothing needs the terminal. The inputs lock while
 * running, since they only apply on the next start. */
function EmbeddedServerSection() {
  const [config, setConfig] = useState<EmbeddedServerConfig>(newConfig(""));
  const [serverNameInput, setServerNameInput] = useState("");
  const [httpsMode, setHttpsMode] = useState<HttpsMode>("none");
  const [status, setStatus] = useState<EmbeddedServerStatus>(STOPPED_STATUS);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<EmbeddedServerConfig | null>("get_embedded_server_config")
      .then(async (saved) => {
        if (saved) {
          setConfig(saved);
          setServerNameInput(saved.serverName ?? "");
          setHttpsMode(saved.tailscaleCert ? "tailscale" : saved.tlsCert ? "own" : "none");
        } else {
          const authKey = await invoke<string>("generate_embedded_server_auth_key");
          setConfig(newConfig(authKey));
        }
      })
      .catch((err) => setError(String(err)));
    invoke<EmbeddedServerStatus>("embedded_server_status")
      .then(setStatus)
      .catch(() => {});
  }, []);

  async function handleGenerateKey() {
    const authKey = await invoke<string>("generate_embedded_server_auth_key");
    setConfig((c) => ({ ...c, authKey }));
  }

  async function handleBrowse(field: "tlsCert" | "tlsKey") {
    const selected = await open({ multiple: false, directory: false });
    if (typeof selected === "string") setConfig((c) => ({ ...c, [field]: selected }));
  }

  async function handleStart() {
    setError(null);
    setBusy(true);
    try {
      // Always save first — `start_embedded_server` reads the config straight from disk, so a
      // field edited here but never saved would otherwise start the server with stale settings.
      const ownCert = httpsMode === "own";
      await invoke("save_embedded_server_config", {
        config: {
          ...config,
          serverName: serverNameInput.trim() || null,
          tailscaleCert: httpsMode === "tailscale",
          tlsCert: ownCert ? config.tlsCert : null,
          tlsKey: ownCert ? config.tlsKey : null,
          tlsHost: ownCert ? config.tlsHost : null,
        },
      });
      const next = await invoke<EmbeddedServerStatus>("start_embedded_server");
      setStatus(next);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleStop() {
    setError(null);
    setBusy(true);
    try {
      await invoke("stop_embedded_server");
      setStatus(STOPPED_STATUS);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("hub.title")}</h3>
      </div>
      <p className="settings-hint">{rich("hub.hint", { server: <code>warden-server</code> })}</p>

      <p className="settings-hint">
        {status.running ? (
          <>
            {rich("hub.runningAs", { name: <strong>{status.serverName}</strong>, addr: <code>{status.boundAddr}</code> })}
            {status.secureUrl
              ? rich("hub.httpsOnly", { url: <code>{status.secureUrl}</code> })
              : status.secure
                ? t("hub.httpsByName")
                : t("hub.notEncrypted")}
            {status.webUrl && (
              <>
                <br />
                {t("hub.webInterface")}
                <a
                  href={status.webUrl}
                  onClick={(e) => {
                    e.preventDefault();
                    void openUrl(status.webUrl!);
                  }}
                >
                  {status.webUrl}
                </a>
                {!status.secureUrl && t("hub.replaceLocalhost")}
              </>
            )}
          </>
        ) : (
          t("hub.stopped")
        )}
      </p>

      <label className="settings-field">
        <span className="settings-label">{t("hub.port")}</span>
        <input
          className="settings-input"
          type="number"
          value={config.port}
          disabled={status.running}
          onChange={(e) => setConfig((c) => ({ ...c, port: Number(e.currentTarget.value) }))}
        />
      </label>
      <label className="settings-field">
        <span className="settings-label">{t("hub.nameOptional")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder={t("hub.namePlaceholder")}
          value={serverNameInput}
          disabled={status.running}
          onChange={(e) => setServerNameInput(e.currentTarget.value)}
        />
      </label>
      <label className="settings-field">
        <span className="settings-label">{t("hub.address")}</span>
        <span className="settings-hint">{rich("hub.addressHint", { all: <code>0.0.0.0</code>, loopback: <code>127.0.0.1</code> })}</span>
        <input
          className="settings-input"
          type="text"
          placeholder="0.0.0.0"
          value={config.listenHost ?? ""}
          disabled={status.running}
          onChange={(e) => setConfig((c) => ({ ...c, listenHost: e.currentTarget.value || null }))}
        />
      </label>
      <label className="settings-field">
        <span className="settings-label">{t("hub.https")}</span>
        <select
          className="settings-select"
          value={httpsMode}
          disabled={status.running}
          onChange={(e) => setHttpsMode(e.currentTarget.value as HttpsMode)}
        >
          <option value="none">{t("hub.httpsNone")}</option>
          <option value="tailscale">{t("hub.httpsTailscale")}</option>
          <option value="own">{t("hub.httpsOwn")}</option>
        </select>
        {httpsMode === "tailscale" && (
          <span className="settings-hint">
            {rich("hub.tailscaleHint", { cmd: <code>tailscale cert</code>, name: <code>*.ts.net</code>, operator: <code>sudo tailscale set --operator=$USER</code> })}
          </span>
        )}
        {httpsMode === "own" && <span className="settings-hint">{t("hub.ownHint")}</span>}
      </label>
      {httpsMode === "own" && (
        <>
          {(["tlsCert", "tlsKey"] as const).map((field) => (
            <label className="settings-field" key={field}>
              <span className="settings-label">{field === "tlsCert" ? t("hub.certificate") : t("hub.privateKey")}</span>
              <div className="settings-key-field">
                <input
                  className="settings-input"
                  type="text"
                  value={config[field] ?? ""}
                  disabled={status.running}
                  onChange={(e) => setConfig((c) => ({ ...c, [field]: e.currentTarget.value || null }))}
                />
                {!status.running && (
                  <button type="button" className="settings-browse-btn" onClick={() => void handleBrowse(field)}>
                    {t("hub.choose")}
                  </button>
                )}
              </div>
            </label>
          ))}
          <label className="settings-field">
            <span className="settings-label">{t("hub.certName")}</span>
            <span className="settings-hint">{rich("hub.certNameHint", { example: <code>hub.mydomain.com</code> })}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="hub.mydomain.com"
              value={config.tlsHost ?? ""}
              disabled={status.running}
              onChange={(e) => setConfig((c) => ({ ...c, tlsHost: e.currentTarget.value || null }))}
            />
          </label>
        </>
      )}
      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={config.webUi}
            disabled={status.running}
            onChange={(e) => setConfig((c) => ({ ...c, webUi: e.currentTarget.checked }))}
          />
          <span className="settings-label">{t("hub.webUi")}</span>
        </span>
        <span className="settings-hint">{t("hub.webUiHint")}</span>
      </label>
      <ApiKeyField label={t("hub.authKey")} value={config.authKey} onChange={(authKey) => setConfig((c) => ({ ...c, authKey }))} />
      {!status.running && (
        <button type="button" className="settings-browse-btn" onClick={handleGenerateKey}>
          {t("hub.generateKey")}
        </button>
      )}

      {error && <p className="settings-error-banner">{error}</p>}

      <button type="button" className="settings-save-btn" onClick={status.running ? handleStop : handleStart} disabled={busy}>
        {busy ? t("hub.wait") : status.running ? t("hub.turnOff") : t("hub.turnOn")}
      </button>
    </section>
  );
}

/** Phase 9.7 — lets the operator save the hub's connection details once and generate a QR a new
 * client (today, the mobile app's `ConnectionScreen`) scans instead of typing them by hand.
 * Doesn't touch `PairingStore`/approval at all — a scanned device still shows up as `pending`
 * above like any other, same as one that connected with hand-typed values. */
/** An invite waiting to be used / just made — mirrors `workspace_cmds::DeviceInviteInfo` and `NewDeviceInvite` (P36). */
interface DeviceInvite {
  name: string;
  expiresMs: number;
}
interface NewDeviceInvite extends DeviceInvite {
  code: string;
}

/** P36 — one-use codes that pair a device without the shared pairing key, and the button that changes that key. Both act
 * on the hub on this machine through its files (the embedded hub and `warden-server serve` read them on every use). */
function DeviceInvitesSection() {
  const [invites, setInvites] = useState<DeviceInvite[]>([]);
  const [name, setName] = useState("");
  const [ttlMinutes, setTtlMinutes] = useState("15");
  const [made, setMade] = useState<NewDeviceInvite | null>(null);
  const [rotatedKey, setRotatedKey] = useState<string | null>(null);
  const [confirmRotate, setConfirmRotate] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function refresh() {
    invoke<DeviceInvite[]>("list_device_invites")
      .then(setInvites)
      .catch((err) => setError(String(err)));
  }
  useEffect(refresh, []);

  async function handleCreate() {
    setError(null);
    setBusy(true);
    try {
      setMade(await invoke<NewDeviceInvite>("create_device_invite", { name, ttlMinutes: Number(ttlMinutes) }));
      setName("");
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleCancel(inviteName: string) {
    setError(null);
    try {
      await invoke("cancel_device_invite", { name: inviteName });
      if (made?.name === inviteName) setMade(null);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleRotate() {
    setError(null);
    setBusy(true);
    try {
      setRotatedKey(await invoke<string>("rotate_pairing_key"));
      setConfirmRotate(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("invites.title")}</h3>
      </div>
      <p className="settings-hint">{t("invites.hint")}</p>
      {error && <p className="settings-error-banner">{error}</p>}

      <label className="settings-field">
        <span className="settings-label">{t("invites.who")}</span>
        <input className="settings-input" type="text" placeholder={t("invites.whoPlaceholder")} value={name} onChange={(e) => setName(e.currentTarget.value)} />
      </label>
      <label className="settings-field">
        <span className="settings-label">{t("invites.minutes")}</span>
        <input
          className="settings-input"
          type="text"
          inputMode="numeric"
          value={ttlMinutes}
          onChange={(e) => setTtlMinutes(e.currentTarget.value)}
        />
      </label>
      <button type="button" className="settings-save-btn" disabled={busy || name.trim() === ""} onClick={handleCreate}>
        {t("invites.create")}
      </button>

      {made && (
        <p className="settings-hint">
          {rich("invites.made", { name: <strong>{made.name}</strong>, date: formatSeen(made.expiresMs), code: <code>{made.code}</code> })}
        </p>
      )}

      {invites.length > 0 && (
        <div className="workspace-device-list">
          {invites.map((invite) => (
            <div className="workspace-device-row" key={invite.name}>
              <div className="workspace-device-info">
                <span className="workspace-device-name">{invite.name}</span>
                <span className="workspace-device-meta">{t("invites.waiting", { date: formatSeen(invite.expiresMs) })}</span>
              </div>
              <div className="workspace-device-actions">
                <button type="button" className="provider-delete-btn" onClick={() => handleCancel(invite.name)}>
                  {t("invites.cancel")}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      <p className="settings-hint">{t("invites.rotateHint")}</p>
      {confirmRotate ? (
        <div className="workspace-device-actions">
          <button type="button" className="provider-delete-btn" disabled={busy} onClick={handleRotate}>
            {t("invites.rotateYes")}
          </button>
          <button type="button" className="settings-browse-btn" onClick={() => setConfirmRotate(false)}>
            {t("invites.keepIt")}
          </button>
        </div>
      ) : (
        <button type="button" className="settings-browse-btn" onClick={() => setConfirmRotate(true)}>
          {t("invites.rotate")}
        </button>
      )}
      {rotatedKey && <p className="settings-hint">{rich("invites.newKey", { key: <code>{rotatedKey}</code> })}</p>}
    </section>
  );
}

function HubPairingQrSection() {
  const [config, setConfig] = useState<HubPairingConfig>({ serverUrl: "" });
  // P36 — the QR carries a one-use invite made when it is drawn, so the screen also keeps when that invite stops working.
  const [qr, setQr] = useState<HubPairingQr | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const [hubs, setHubs] = useState<DiscoveredHub[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [discoveryPort, setDiscoveryPort] = useState("7420");
  // P36 — the embedded hub's own wss:// URL, offered as a one-click Server URL when it's on.
  const [embeddedSecureUrl, setEmbeddedSecureUrl] = useState<string | null>(null);

  useEffect(() => {
    invoke<HubPairingConfig | null>("get_hub_pairing_config")
      .then((saved) => saved && setConfig(saved))
      .catch((err) => setError(String(err)));
    invoke<EmbeddedServerStatus>("embedded_server_status")
      .then((status) => setEmbeddedSecureUrl(status.secureUrl))
      .catch(() => {});
  }, []);

  async function handleGenerate() {
    setError(null);
    setBusy(true);
    setQr(null);
    try {
      await invoke("save_hub_pairing_config", { serverUrl: config.serverUrl });
      setQr(await invoke<HubPairingQr>("hub_pairing_qr_svg"));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  // Phase 9.1 (redefined) — sweeps the LAN instead of asking the operator to already know the IP.
  // Only ever fills Server URL: the probe's reply has no secret in it.
  async function handleDiscover() {
    setSearchError(null);
    setSearching(true);
    setHubs(null);
    try {
      const port = Number(discoveryPort);
      const found = await invoke<DiscoveredHub[]>("discover_hubs", { port });
      setHubs(found);
    } catch (err) {
      setSearchError(String(err));
    } finally {
      setSearching(false);
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("qr.title")}</h3>
      </div>
      <p className="settings-hint">{rich("qr.hint", { server: <code>warden-server</code> })}</p>

      <label className="settings-field">
        <span className="settings-label">{t("qr.port")}</span>
        <input
          className="settings-input"
          type="text"
          inputMode="numeric"
          placeholder="7420"
          value={discoveryPort}
          onChange={(e) => setDiscoveryPort(e.currentTarget.value)}
        />
      </label>
      <button type="button" className="settings-save-btn" onClick={handleDiscover} disabled={searching}>
        {searching ? t("qr.searching") : t("qr.search")}
      </button>
      {searchError && <p className="settings-error-banner">{searchError}</p>}
      {hubs && hubs.length === 0 && <p className="settings-hint">{t("qr.noneFound")}</p>}
      {hubs && hubs.length > 0 && (
        <div className="workspace-device-list">
          {hubs.map((hub) => (
            <button
              type="button"
              key={`${hub.host}:${hub.port}`}
              className="workspace-device-row workspace-device-row--clickable"
              onClick={() => setConfig((c) => ({ ...c, serverUrl: hub.secureUrl ?? `ws://${hub.host}:${hub.port}` }))}
            >
              <div className="workspace-device-info">
                <span className="workspace-device-name">{hub.serverName}</span>
                <span className="workspace-device-meta">{hub.secureUrl ?? `${hub.host}:${hub.port}`}</span>
              </div>
            </button>
          ))}
        </div>
      )}

      <label className="settings-field">
        <span className="settings-label">{t("qr.serverUrl")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder={t("qr.serverUrlPlaceholder")}
          value={config.serverUrl}
          onChange={(e) => setConfig((c) => ({ ...c, serverUrl: e.currentTarget.value }))}
        />
      </label>
      {embeddedSecureUrl && config.serverUrl !== embeddedSecureUrl && (
        <button type="button" className="settings-browse-btn" onClick={() => setConfig((c) => ({ ...c, serverUrl: embeddedSecureUrl }))}>
          {t("qr.useThis", { url: embeddedSecureUrl })}
        </button>
      )}

      {error && <p className="settings-error-banner">{error}</p>}

      <button type="button" className="settings-save-btn" onClick={handleGenerate} disabled={busy}>
        {t("qr.generate")}
      </button>

      {qr && (
        <div className="sync-qr-card">
          <div className="sync-qr-image" dangerouslySetInnerHTML={{ __html: qr.svg }} />
          <p className="settings-hint">{t("qr.worksOnce", { time: new Date(qr.expiresMs).toLocaleTimeString(intlTag()) })}</p>
        </div>
      )}
    </section>
  );
}

/** Mirrors `UserInfoDto` (P84). */
interface Person {
  id: string;
  name: string;
  role: string;
  mustChangePassword: boolean;
  /** Fatia 2 — the tools you set for them; absent/null is the safe default. */
  tools?: string[] | null;
  /** Their own agents' names. */
  agents?: string[];
  /** Fatia 5 — the TruthID username they linked, and whether an invite is still open. */
  truthid?: string;
  inviteOpen?: boolean;
}

/** Mirrors `warden_bootstrap::users::default_member_tool`: what a member has when you never chose. */
function safeByDefault(tool: string): boolean {
  return ["read_file", "write_file", "use_skill", "read_skill_file", "manage_skill", "delegate_task", "jobs", "budget", "generate_document", "search_history"].includes(tool) || tool.startsWith("tavily");
}

/** Mirrors `NEVER_FOR_MEMBERS`: never theirs, so never offered. */
const NEVER_FOR_MEMBERS = ["delegate_to_agent", "message_agent", "manage_agents", "manage_tasks", "usage_stats"];

interface PeoplePayload {
  users: Person[];
  tempPassword: string | null;
  inviteCode: string | null;
}

type PeopleDraft =
  | { kind: "add"; id: string; name: string }
  | { kind: "rename"; id: string; name: string }
  /** `null`: the safe default. */
  | { kind: "tools"; id: string; tools: string[] | null };

/**
 * P84 — the members of this workspace besides you: `[[users]]` in config.toml, which syncs, so someone
 * added here can sign in on the hub on your VPS too once the file gets there. Each signs in on the web
 * or the phone with a username and password, and has their own vault and conversations. A provisional
 * password is shown once; they pick their own on first sign-in.
 */
function PeopleSection() {
  const [people, setPeople] = useState<Person[] | null>(null);
  const [draft, setDraft] = useState<PeopleDraft | null>(null);
  const [shown, setShown] = useState<{ id: string; password: string } | null>(null);
  const [invite, setInvite] = useState<{ id: string; code: string } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toolNames, setToolNames] = useState<string[]>([]);

  useEffect(() => {
    invoke<PeoplePayload>("list_people")
      .then((p) => setPeople(p.users))
      .catch((err) => setError(String(err)));
    invoke<string[]>("list_tool_names")
      .then((names) => setToolNames(names.filter((n) => !NEVER_FOR_MEMBERS.includes(n))))
      .catch(() => setToolNames([]));
  }, []);

  async function run(command: string, args: Record<string, unknown>, shownFor?: string) {
    setError(null);
    try {
      const payload = await invoke<PeoplePayload>(command, args);
      setPeople(payload.users);
      if (payload.tempPassword && shownFor) setShown({ id: shownFor, password: payload.tempPassword });
      if (payload.inviteCode && shownFor) setInvite({ id: shownFor, code: payload.inviteCode });
      setDraft(null);
      setConfirmRemove(null);
    } catch (err) {
      setError(String(err));
    }
  }

  async function save() {
    if (!draft) return;
    if (draft.kind === "add") {
      const id = draft.id.trim().toLowerCase();
      await run("add_person", { id, name: draft.name }, id);
    } else if (draft.kind === "tools") {
      await run("set_person_tools", { id: draft.id, tools: draft.tools });
    } else {
      await run("rename_person", { id: draft.id, name: draft.name });
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("people.title")}</h3>
        <button type="button" className="settings-browse-btn" disabled={draft !== null} onClick={() => setDraft({ kind: "add", id: "", name: "" })}>
          {t("people.add")}
        </button>
      </div>
      <p className="settings-hint">{t("people.hint")}</p>
      {error && <p className="settings-error-banner">{error}</p>}
      {shown && (
        <div className="provider-card">
          <p className="settings-hint">{rich("people.provisional", { id: <strong>{shown.id}</strong>, password: <code>{shown.password}</code> })}</p>
          <p className="settings-hint">{t("people.giveIt")}</p>
          <button type="button" className="settings-browse-btn" onClick={() => setShown(null)}>
            {t("people.done")}
          </button>
        </div>
      )}

      {invite && (
        <div className="provider-card">
          <p className="settings-hint">{rich("people.invite", { id: <strong>{invite.id}</strong>, code: <code>{invite.code}</code> })}</p>
          <p className="settings-hint">{t("people.inviteHint")}</p>
          <button type="button" className="settings-browse-btn" onClick={() => setInvite(null)}>
            {t("people.done")}
          </button>
        </div>
      )}

      {draft?.kind === "tools" && (
        <div className="provider-card skill-editor">
          <span className="settings-label">{t("people.toolsFor", { id: draft.id })}</span>
          <span className="settings-checkbox-row">
            <input
              type="checkbox"
              id="person-tools-default"
              checked={draft.tools === null}
              onChange={(e) => setDraft({ ...draft, tools: e.currentTarget.checked ? null : toolNames.filter(safeByDefault) })}
            />
            <label htmlFor="person-tools-default">{t("people.safeDefault")}</label>
          </span>
          {draft.tools !== null &&
            toolNames.map((tool) => (
              <span key={tool} className="settings-checkbox-row">
                <input
                  type="checkbox"
                  id={`person-tool-${tool}`}
                  checked={draft.tools!.includes(tool)}
                  onChange={(e) => setDraft({ ...draft, tools: e.currentTarget.checked ? [...draft.tools!, tool] : draft.tools!.filter((t) => t !== tool) })}
                />
                <label htmlFor={`person-tool-${tool}`}>
                  {tool}
                  {!safeByDefault(tool) && <span className="settings-hint">{t("people.reachesYours")}</span>}
                </label>
              </span>
            ))}
          <span className="settings-hint">{t("people.neverPast")}</span>
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={() => void save()}>
              {t("people.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)}>
              {t("people.cancel")}
            </button>
          </div>
        </div>
      )}

      {draft && draft.kind !== "tools" && (
        <div className="provider-card skill-editor">
          {draft.kind === "add" && (
            <label className="settings-field">
              <span className="settings-label">{t("people.username")}</span>
              <input className="settings-input" type="text" placeholder="ana" value={draft.id} onChange={(e) => setDraft({ ...draft, id: e.currentTarget.value })} />
              <span className="settings-hint">{t("people.usernameHint")}</span>
            </label>
          )}
          <label className="settings-field">
            <span className="settings-label">{t("people.name")}</span>
            <input className="settings-input" type="text" placeholder="Ana Souza" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.currentTarget.value })} />
          </label>
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" disabled={draft.name.trim() === "" || (draft.kind === "add" && draft.id.trim() === "")} onClick={() => void save()}>
              {draft.kind === "add" ? t("people.addButton") : t("people.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)}>
              {t("people.cancel")}
            </button>
          </div>
        </div>
      )}

      {people === null ? (
        <p className="settings-hint">{t("people.loading")}</p>
      ) : people.length === 0 ? (
        <p className="settings-hint">{t("people.justYou")}</p>
      ) : (
        <div className="workspace-device-list">
          {people.map((person) => (
            <div className="workspace-device-row" key={person.id}>
              <div className="workspace-device-info">
                <div className="workspace-device-name-row">
                  <span className="workspace-device-name">{person.name}</span>
                  <span className={`storage-provider-badge workspace-status-badge workspace-status-badge--${person.mustChangePassword ? "pending" : "approved"}`}>
                    {person.mustChangePassword ? t("people.statusProvisional") : t("people.statusActive")}
                  </span>
                </div>
                <span className="workspace-device-meta">
                  {person.id} · {person.tools == null ? t("people.defaultTools") : person.tools.length === 0 ? t("people.noTools") : t("people.toolsList", { list: person.tools.join(", ") })}
                  {person.agents && person.agents.length > 0 ? t("people.ownAgents", { list: person.agents.join(", ") }) : ""}
                  {person.truthid ? t("people.truthid", { name: person.truthid }) : person.inviteOpen ? t("people.inviteOpen") : ""}
                </span>
                {confirmRemove === person.id && <span className="settings-hint">{t("people.confirmRemove", { name: person.name })}</span>}
              </div>
              <div className="workspace-device-actions">
                {confirmRemove === person.id ? (
                  <>
                    <button type="button" className="provider-delete-btn" onClick={() => void run("remove_person", { id: person.id })}>
                      {t("people.remove")}
                    </button>
                    <button type="button" className="settings-browse-btn" onClick={() => setConfirmRemove(null)}>
                      {t("people.cancel")}
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="settings-browse-btn" disabled={draft !== null} onClick={() => setDraft({ kind: "rename", id: person.id, name: person.name })}>
                      {t("people.rename")}
                    </button>
                    <button type="button" className="settings-browse-btn" disabled={draft !== null} onClick={() => setDraft({ kind: "tools", id: person.id, tools: person.tools ?? null })}>
                      {t("people.tools")}
                    </button>
                    {person.truthid || person.inviteOpen ? (
                      <button type="button" className="settings-browse-btn" onClick={() => void run("unlink_person_truthid", { id: person.id })}>
                        {person.truthid ? t("people.unlink") : t("people.cancelInvite")}
                      </button>
                    ) : (
                      <button type="button" className="settings-browse-btn" onClick={() => void run("invite_person", { id: person.id }, person.id)}>
                        {t("people.truthidInvite")}
                      </button>
                    )}
                    <button type="button" className="settings-browse-btn" onClick={() => void run("reset_person_password", { id: person.id }, person.id)}>
                      {t("people.newPassword")}
                    </button>
                    <button type="button" className="provider-delete-btn" onClick={() => setConfirmRemove(person.id)}>
                      {t("people.remove")}
                    </button>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      <SharedSpacesSection people={people ?? []} />
    </section>
  );
}

/** Mirrors `SpaceDto` (P84 slice 3). */
interface SharedSpace {
  id: string;
  folder: string;
  /** Usernames, or `"*"` for everyone. */
  readers: string[];
  /** Writers also read. */
  writers: string[];
}

const EVERYONE = "*";

type SpaceAccess = "none" | "read" | "write";

function spaceAccess(space: SharedSpace, who: string): SpaceAccess {
  if (space.writers.includes(who)) return "write";
  if (space.readers.includes(who)) return "read";
  return "none";
}

function withSpaceAccess(space: SharedSpace, who: string, access: SpaceAccess): SharedSpace {
  const readers = space.readers.filter((r) => r !== who);
  const writers = space.writers.filter((w) => w !== who);
  if (access === "read") readers.push(who);
  if (access === "write") writers.push(who);
  return { ...space, readers, writers };
}

/**
 * P84 slice 3 — folders of your vault that members see inside theirs, at `compartilhado/<name>/`:
 * `[[spaces]]` in config.toml, synced like the people above. An agent talking to a member reads (and,
 * if you let it, writes) only in these folders, never the rest of your vault.
 */
function SharedSpacesSection({ people }: { people: Person[] }) {
  const [spaces, setSpaces] = useState<SharedSpace[] | null>(null);
  /** `originalId` absent: a new space. */
  const [draft, setDraft] = useState<{ originalId?: string; space: SharedSpace } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<SharedSpace[]>("list_shared_spaces")
      .then(setSpaces)
      .catch((err) => setError(String(err)));
  }, []);

  async function run(command: string, args: Record<string, unknown>) {
    setError(null);
    try {
      setSpaces(await invoke<SharedSpace[]>(command, args));
      setDraft(null);
      setConfirmRemove(null);
    } catch (err) {
      setError(String(err));
    }
  }

  const name = (id: string) => (id === EVERYONE ? t("spaces.everyone") : (people.find((p) => p.id === id)?.name ?? id));
  const who = (space: SharedSpace) => {
    const parts = [];
    if (space.writers.length > 0) parts.push(t("spaces.write", { names: space.writers.map(name).join(", ") }));
    if (space.readers.length > 0) parts.push(t("spaces.read", { names: space.readers.map(name).join(", ") }));
    return parts.length > 0 ? parts.join(" · ") : t("spaces.noOne");
  };

  const accessSelect = (space: SharedSpace, person: string) => (
    <select className="settings-input" value={spaceAccess(space, person)} onChange={(e) => draft && setDraft({ ...draft, space: withSpaceAccess(space, person, e.currentTarget.value as SpaceAccess) })}>
      <option value="none">{t("spaces.none")}</option>
      <option value="read">{t("spaces.reads")}</option>
      <option value="write">{t("spaces.writes")}</option>
    </select>
  );

  return (
    <>
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("spaces.title")}</h3>
        <button
          type="button"
          className="settings-browse-btn"
          disabled={draft !== null || people.length === 0}
          onClick={() => setDraft({ space: { id: "", folder: "", readers: [], writers: [] } })}
        >
          {t("spaces.add")}
        </button>
      </div>
      <p className="settings-hint">{rich("spaces.hint", { folder: <code>compartilhado/</code> })}</p>
      {error && <p className="settings-error-banner">{error}</p>}

      {draft && (
        <div className="provider-card skill-editor">
          <label className="settings-field">
            <span className="settings-label">{t("spaces.name")}</span>
            <input className="settings-input" type="text" placeholder="casa" value={draft.space.id} onChange={(e) => setDraft({ ...draft, space: { ...draft.space, id: e.currentTarget.value } })} />
            <span className="settings-hint">{t("spaces.nameHint", { name: draft.space.id.trim().toLowerCase() || t("spaces.nameFallback") })}</span>
          </label>
          <label className="settings-field">
            <span className="settings-label">{t("spaces.folder")}</span>
            <input className="settings-input" type="text" placeholder="casa" value={draft.space.folder} onChange={(e) => setDraft({ ...draft, space: { ...draft.space, folder: e.currentTarget.value } })} />
            <span className="settings-hint">{t("spaces.folderHint")}</span>
          </label>
          <label className="settings-field">
            <span className="settings-label">{t("spaces.everyoneLabel")}</span>
            {accessSelect(draft.space, EVERYONE)}
          </label>
          {people.map((person) => (
            <label className="settings-field" key={person.id}>
              <span className="settings-label">{person.name}</span>
              {accessSelect(draft.space, person.id)}
            </label>
          ))}
          <div className="skill-editor-actions">
            <button
              type="button"
              className="settings-save-btn"
              disabled={draft.space.id.trim() === "" || draft.space.folder.trim() === ""}
              onClick={() => void run("save_shared_space", { originalId: draft.originalId ?? null, space: { ...draft.space, id: draft.space.id.trim().toLowerCase(), folder: draft.space.folder.trim() } })}
            >
              {draft.originalId ? t("spaces.save") : t("spaces.share")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)}>
              {t("spaces.cancel")}
            </button>
          </div>
        </div>
      )}

      {spaces === null ? (
        <p className="settings-hint">{t("spaces.loading")}</p>
      ) : spaces.length === 0 ? (
        <p className="settings-hint">{people.length === 0 ? t("spaces.addFirst") : t("spaces.noneShared")}</p>
      ) : (
        <div className="workspace-device-list">
          {spaces.map((space) => (
            <div className="workspace-device-row" key={space.id}>
              <div className="workspace-device-info">
                <div className="workspace-device-name-row">
                  <span className="workspace-device-name">{space.id}</span>
                  <code>{space.folder}</code>
                </div>
                <span className="workspace-device-meta">{who(space)}</span>
                {confirmRemove === space.id && (
                  <span className="settings-hint">{t("spaces.confirmStop")}</span>
                )}
              </div>
              <div className="workspace-device-actions">
                {confirmRemove === space.id ? (
                  <>
                    <button type="button" className="provider-delete-btn" onClick={() => void run("remove_shared_space", { id: space.id })}>
                      {t("spaces.stop")}
                    </button>
                    <button type="button" className="settings-browse-btn" onClick={() => setConfirmRemove(null)}>
                      {t("spaces.cancel")}
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="settings-browse-btn" disabled={draft !== null} onClick={() => setDraft({ originalId: space.id, space })}>
                      {t("spaces.edit")}
                    </button>
                    <button type="button" className="provider-delete-btn" onClick={() => setConfirmRemove(space.id)}>
                      {t("spaces.stop")}
                    </button>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </>
  );
}

/** Mirrors `LendConfigPayload` (P97). */
interface LendConfig {
  hubUrl: string;
  name: string;
  description: string;
  tags: string[];
  shell: boolean;
  files: string | null;
  mcp: string[];
  models: string[];
}

/** Mirrors `NodeState` (`warden_server::node_client`). */
type LendState =
  | { state: "connecting" }
  | { state: "connected" }
  | { state: "retrying"; error: string; inSecs: number }
  | { state: "stopped"; error: string };

interface LendActivity {
  at: number;
  kind: string;
  summary: string;
  error: string | null;
}

interface LendStatus {
  config: LendConfig | null;
  enabled: boolean;
  state: LendState | null;
  deviceId: string | null;
  paired: boolean;
  activity: LendActivity[];
}

interface LendOptions {
  mcpServers: string[];
  models: string[];
  defaultName: string;
}

const EMPTY_LEND: LendConfig = { hubUrl: "", name: "", description: "", tags: [], shell: false, files: null, mcp: [], models: [] };

function lendStateLabel(state: LendState | null): string {
  if (!state) return t("lend.off");
  switch (state.state) {
    case "connecting":
      return t("lend.connecting");
    case "connected":
      return t("lend.connected");
    case "retrying":
      return t("lend.retrying", { secs: state.inSecs, error: state.error });
    case "stopped":
      return t("lend.stopped", { error: state.error });
  }
}

function toggle(list: string[], item: string, on: boolean): string[] {
  return on ? [...list, item] : list.filter((x) => x !== item);
}

/**
 * P97 — this computer as a node of another hub (the one on your VPS): the same thing as
 * `warden-server node`, from here. You pick what it lends; the hub still has to approve it as a
 * device and allow it for its agents. Saved in hub-local.json, which doesn't sync, so it's only on
 * for this computer. The pairing key is used once and never saved.
 */
function LendSection() {
  const [status, setStatus] = useState<LendStatus | null>(null);
  const [options, setOptions] = useState<LendOptions>({ mcpServers: [], models: [], defaultName: "" });
  const [form, setForm] = useState<LendConfig>(EMPTY_LEND);
  const [tagsText, setTagsText] = useState("");
  const [authKey, setAuthKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function apply(next: LendStatus) {
    setStatus(next);
    if (next.config) {
      setForm(next.config);
      setTagsText(next.config.tags.join(", "));
    }
  }

  useEffect(() => {
    invoke<LendStatus>("get_lend_status")
      .then(apply)
      .catch((err) => setError(String(err)));
    invoke<LendOptions>("lend_options")
      .then(setOptions)
      .catch(() => {});
  }, []);

  const running = status?.state != null;

  // While on: the state and the activity move on their own.
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      invoke<LendStatus>("get_lend_status")
        .then(setStatus)
        .catch(() => {});
    }, 3000);
    return () => window.clearInterval(timer);
  }, [running]);

  async function start() {
    setError(null);
    setBusy(true);
    try {
      const config = { ...form, tags: tagsText.split(",").map((t) => t.trim()).filter(Boolean) };
      apply(await invoke<LendStatus>("start_lending", { config, authKey: authKey.trim() || null }));
      setAuthKey("");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setError(null);
    try {
      apply(await invoke<LendStatus>("stop_lending"));
    } catch (err) {
      setError(String(err));
    }
  }

  async function pickFolder() {
    const selected = await open({ multiple: false, directory: true });
    if (typeof selected === "string") setForm((f) => ({ ...f, files: selected }));
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("lend.title")}</h3>
        {running ? (
          <button type="button" className="provider-delete-btn" onClick={() => void stop()}>
            {t("lend.stop")}
          </button>
        ) : (
          <button type="button" className="settings-save-btn" disabled={busy || form.hubUrl.trim() === ""} onClick={() => void start()}>
            {busy ? t("lend.starting") : t("lend.start")}
          </button>
        )}
      </div>
      <p className="settings-hint">{t("lend.hint")}</p>
      {error && <p className="settings-error-banner">{error}</p>}
      {status && (
        <p className="settings-hint">
          <strong>{lendStateLabel(status.state)}</strong>
          {status.deviceId ? t("lend.deviceId", { id: status.deviceId }) : ""}
        </p>
      )}

      <div className="provider-card skill-editor">
        <label className="settings-field">
          <span className="settings-label">{t("lend.hubAddress")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder="wss://my-vps.tailnet.ts.net:7420"
            value={form.hubUrl}
            disabled={running}
            onChange={(e) => setForm({ ...form, hubUrl: e.currentTarget.value })}
          />
        </label>
        {!running && (
          <>
            <ApiKeyField label={status?.paired ? t("lend.pairingAgain") : t("lend.pairingFirst")} value={authKey} onChange={setAuthKey} />
            <span className="settings-hint">{t("lend.pairingHint")}</span>
          </>
        )}
        <label className="settings-field">
          <span className="settings-label">{t("lend.name")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder={options.defaultName}
            value={form.name}
            disabled={running}
            onChange={(e) => setForm({ ...form, name: e.currentTarget.value })}
          />
        </label>
        <label className="settings-field">
          <span className="settings-label">{t("lend.description")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder={t("lend.descriptionPlaceholder")}
            value={form.description}
            disabled={running}
            onChange={(e) => setForm({ ...form, description: e.currentTarget.value })}
          />
        </label>
        <label className="settings-field">
          <span className="settings-label">{t("lend.tags")}</span>
          <input className="settings-input" type="text" placeholder="home, gpu" value={tagsText} disabled={running} onChange={(e) => setTagsText(e.currentTarget.value)} />
        </label>
        <label className="settings-field settings-checkbox-field">
          <span className="settings-checkbox-row">
            <input type="checkbox" checked={form.shell} disabled={running} onChange={(e) => setForm({ ...form, shell: e.currentTarget.checked })} />
            <span className="settings-label">{t("lend.shell")}</span>
          </span>
          <span className="settings-hint">{t("lend.shellHint")}</span>
        </label>
        <div className="settings-field">
          <span className="settings-label">{t("lend.folder")}</span>
          <div className="settings-checkbox-row">
            <input className="settings-input" type="text" readOnly placeholder={t("lend.none")} value={form.files ?? ""} />
            <button type="button" className="settings-browse-btn" disabled={running} onClick={() => void pickFolder()}>
              {t("hub.choose")}
            </button>
            {form.files && (
              <button type="button" className="settings-browse-btn" disabled={running} onClick={() => setForm({ ...form, files: null })}>
                {t("lend.clear")}
              </button>
            )}
          </div>
        </div>
        {options.mcpServers.length > 0 && (
          <div className="settings-field">
            <span className="settings-label">{t("lend.mcp")}</span>
            <div className="skill-agent-list">
              {options.mcpServers.map((name) => (
                <label className="skill-agent-option" key={name}>
                  <input type="checkbox" checked={form.mcp.includes(name)} disabled={running} onChange={(e) => setForm({ ...form, mcp: toggle(form.mcp, name, e.currentTarget.checked) })} />
                  {name}
                </label>
              ))}
            </div>
          </div>
        )}
        {options.models.length > 0 && (
          <div className="settings-field">
            <span className="settings-label">{t("lend.models")}</span>
            <div className="skill-agent-list">
              {options.models.map((id) => (
                <label className="skill-agent-option" key={id}>
                  <input type="checkbox" checked={form.models.includes(id)} disabled={running} onChange={(e) => setForm({ ...form, models: toggle(form.models, id, e.currentTarget.checked) })} />
                  {id}
                </label>
              ))}
            </div>
            <span className="settings-hint">{t("lend.modelsHint")}</span>
          </div>
        )}
      </div>

      {running && (
        <>
          <h4 className="settings-label">{t("lend.did")}</h4>
          {status.activity.length === 0 ? (
            <p className="settings-hint">{t("lend.nothing")}</p>
          ) : (
            <div className="workspace-device-list">
              {status.activity.map((entry, i) => (
                <div className="workspace-device-row" key={`${entry.at}-${i}`}>
                  <div className="workspace-device-info">
                    <span className="workspace-device-name">
                      {entry.kind} {entry.summary && <code>{entry.summary}</code>}
                    </span>
                    <span className="workspace-device-meta">
                      {formatSeen(entry.at * 1000)}
                      {entry.error ? t("lend.failed", { error: entry.error }) : ""}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}

/** Mirrors `NodeInfoDto` (P93). */
interface NodeInfo {
  deviceId: string;
  name: string;
  online: boolean;
  approved: boolean;
  offer?: { description: string; tags: string[]; shell: boolean; files: boolean; mcpTools?: { name: string }[]; models?: string[] };
  enabled: boolean;
  agents: string[];
  requireApproval: boolean;
}

interface NodeDraft {
  deviceId: string;
  enabled: boolean;
  agents: string[];
  requireApproval: boolean;
}

function offerLabel(node: NodeInfo): string {
  if (!node.offer) return t("nodes.notConnected");
  const mcp = node.offer.mcpTools ?? [];
  const parts = [
    node.offer.shell && t("nodes.itsShell"),
    node.offer.files && t("nodes.aFolder"),
    mcp.length > 0 && t("nodes.mcpTools", { count: mcp.length, names: mcp.map((tool) => tool.name).join(", ") }),
    (node.offer.models ?? []).length > 0 && t("nodes.models", { list: (node.offer.models ?? []).join(", ") }),
  ].filter(Boolean);
  return parts.length ? t("nodes.lends", { parts: parts.join(t("nodes.and")) }) : t("nodes.lendsNothing");
}

/**
 * P93 — machines running `warden-server node` that lend their shell or a folder to your agents. Two
 * locks: the node's operator chose what it offers; here you pick whether agents may use it, which
 * ones, and whether each call asks you first. It must also be approved in the device list below. The
 * entries are saved in config.toml, which syncs — so a node that joins the hub on your VPS can be
 * allowed from here by typing its id.
 */
function NodesSection({ refreshKey }: { refreshKey: number }) {
  const [nodes, setNodes] = useState<NodeInfo[] | null>(null);
  const [agentIds, setAgentIds] = useState<string[]>([]);
  const [draft, setDraft] = useState<NodeDraft | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<NodeInfo[]>("list_nodes")
      .then(setNodes)
      .catch((err) => setError(String(err)));
    invoke<{ agents: { id: string }[] }>("get_settings")
      .then((settings) => setAgentIds(settings.agents.map((a) => a.id)))
      .catch(() => setAgentIds([]));
  }, [refreshKey]);

  async function save() {
    if (!draft) return;
    setError(null);
    try {
      setNodes(await invoke<NodeInfo[]>("save_node_access", { ...draft }));
      setDraft(null);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("nodes.title")}</h3>
        <button type="button" className="settings-browse-btn" onClick={() => setDraft({ deviceId: "", enabled: true, agents: [], requireApproval: false })}>
          {t("nodes.allow")}
        </button>
      </div>
      <p className="settings-hint">{rich("nodes.hint", { command: <code>warden-server node --hub … --shell --files &lt;folder&gt;</code> })}</p>
      {error && <p className="settings-error-banner">{error}</p>}

      {draft && (
        <div className="provider-card skill-editor">
          <label className="settings-field">
            <span className="settings-label">{t("nodes.deviceId")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="node-home-pc-1a2b3c4d"
              value={draft.deviceId}
              disabled={nodes?.some((n) => n.deviceId === draft.deviceId) && draft.deviceId !== ""}
              onChange={(e) => setDraft({ ...draft, deviceId: e.currentTarget.value })}
            />
          </label>
          <label className="settings-field settings-checkbox-field">
            <span className="settings-checkbox-row">
              <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.currentTarget.checked })} />
              <span className="settings-label">{t("nodes.mayUse")}</span>
            </span>
          </label>
          <div className="settings-field">
            <span className="settings-label">{t("nodes.onlyThese")}</span>
            <div className="skill-agent-list">
              {agentIds.map((id) => (
                <label className="skill-agent-option" key={id}>
                  <input
                    type="checkbox"
                    checked={draft.agents.includes(id)}
                    onChange={(e) =>
                      setDraft({ ...draft, agents: e.currentTarget.checked ? [...draft.agents, id] : draft.agents.filter((a) => a !== id) })
                    }
                  />
                  {id}
                </label>
              ))}
            </div>
            <span className="settings-hint">{t("nodes.noneTicked")}</span>
          </div>
          <label className="settings-field settings-checkbox-field">
            <span className="settings-checkbox-row">
              <input type="checkbox" checked={draft.requireApproval} onChange={(e) => setDraft({ ...draft, requireApproval: e.currentTarget.checked })} />
              <span className="settings-label">{t("nodes.askFirst")}</span>
            </span>
            <span className="settings-hint">{t("nodes.askFirstHint")}</span>
          </label>
          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" disabled={draft.deviceId.trim() === ""} onClick={() => void save()}>
              {t("nodes.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setDraft(null)}>
              {t("nodes.cancel")}
            </button>
          </div>
        </div>
      )}

      {nodes === null ? (
        <p className="settings-hint">{t("nodes.loading")}</p>
      ) : nodes.length === 0 ? (
        <p className="settings-hint">{t("nodes.none")}</p>
      ) : (
        <div className="workspace-device-list">
          {nodes.map((node) => (
            <div className="workspace-device-row" key={node.deviceId}>
              <div className="workspace-device-info">
                <div className="workspace-device-name-row">
                  <span className="workspace-device-name">
                    {node.name} {node.online ? t("nodes.online") : ""}
                  </span>
                  <span className={`storage-provider-badge workspace-status-badge workspace-status-badge--${node.enabled && node.approved ? "approved" : "pending"}`}>
                    {!node.approved ? t("nodes.needsApproval") : node.enabled ? t("nodes.allowed") : t("nodes.blocked")}
                  </span>
                </div>
                <span className="workspace-device-meta">
                  {node.deviceId} · {offerLabel(node)}
                  {node.offer?.description ? ` · ${node.offer.description}` : ""}
                  {node.offer && node.offer.tags.length > 0 ? ` · ${node.offer.tags.join(", ")}` : ""}
                  {" · "}
                  {node.agents.length === 0 ? t("nodes.everyAgent") : t("nodes.onlyAgents", { list: node.agents.join(", ") })}
                  {node.requireApproval ? t("nodes.asksFirst") : ""}
                </span>
              </div>
              <div className="workspace-device-actions">
                <button
                  type="button"
                  className="settings-browse-btn"
                  onClick={() => setDraft({ deviceId: node.deviceId, enabled: node.enabled, agents: node.agents, requireApproval: node.requireApproval })}
                >
                  {t("nodes.access")}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

function WorkspaceView() {
  const [devices, setDevices] = useState<PairedDevice[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [pendingActionId, setPendingActionId] = useState<string | null>(null);
  /** Bumped with the device list, so the nodes reread their "approved" state. */
  const [refreshKey, setRefreshKey] = useState(0);

  function refresh() {
    invoke<PairedDevice[]>("list_paired_devices")
      .then((next) => {
        setDevices(next);
        setRefreshKey((k) => k + 1);
      })
      .catch((err) => setError(String(err)));
  }

  // Fetched fresh every time this view mounts, same as `UsageView`/`SettingsView` — pairing state
  // can change from the CLI (`warden-server devices approve/revoke`) between visits.
  useEffect(refresh, []);

  async function runAction(command: "approve_paired_device" | "revoke_paired_device", deviceId: string) {
    setActionError(null);
    setPendingActionId(deviceId);
    try {
      await invoke(command, { deviceId });
      refresh();
    } catch (err) {
      setActionError(String(err));
    } finally {
      setPendingActionId(null);
    }
  }

  if (error) {
    return (
      <div className="settings-view">
        <h2 className="settings-title">{t("workspace.title")}</h2>
        <p className="usage-error">{error}</p>
      </div>
    );
  }

  if (!devices) {
    return (
      <div className="settings-view">
        <p>{t("workspace.loading")}</p>
      </div>
    );
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("workspace.title")}</h2>
      <p className="settings-hint">
        {rich("workspace.hint", { server: <code>warden-server</code>, here: <strong>{t("workspace.thisMachine")}</strong>, routing: <code>CallDeviceTool</code> })}
      </p>

      <EmbeddedServerSection />

      <HubsSection />

      <DeviceInvitesSection />

      <HubPairingQrSection />

      {actionError && <p className="settings-error-banner">{actionError}</p>}

      {devices.length === 0 ? (
        <p className="settings-hint">{t("workspace.noDevices")}</p>
      ) : (
        <div className="workspace-device-list">
          {devices.map((device) => (
            <div className="workspace-device-row" key={device.deviceId}>
              <div className="workspace-device-info">
                <div className="workspace-device-name-row">
                  <span className="workspace-device-name">{device.deviceName}</span>
                  <StatusBadge status={device.status} />
                </div>
                <span className="workspace-device-meta">
                  {t("workspace.deviceMeta", { id: device.deviceId, first: formatSeen(device.firstSeenMs), last: formatSeen(device.lastSeenMs) })}
                </span>
              </div>
              <div className="workspace-device-actions">
                {device.status === "pending" && (
                  <button
                    type="button"
                    className="settings-save-btn"
                    disabled={pendingActionId === device.deviceId}
                    onClick={() => runAction("approve_paired_device", device.deviceId)}
                  >
                    {t("workspace.approve")}
                  </button>
                )}
                {device.status === "approved" && (
                  <button
                    type="button"
                    className="provider-delete-btn"
                    disabled={pendingActionId === device.deviceId}
                    onClick={() => runAction("revoke_paired_device", device.deviceId)}
                  >
                    {t("workspace.revoke")}
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      <PeopleSection />
      <LendSection />
      <NodesSection refreshKey={refreshKey} />
    </div>
  );
}

export default WorkspaceView;
