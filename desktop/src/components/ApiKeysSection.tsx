import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";
import type { EmbeddedServerStatus, Settings } from "../types";
import { canEditRaw, hasAll, missingIds, RAW_ALL, rawOptions, rawSummary, toggleRaw, type RawOption } from "../lib/rawAccess";

/** Mirrors `api_key_cmds::ApiKeyInfo` — never the key or its hash; `shown` is its start. */
interface ApiKeyInfo {
  id: string;
  name: string;
  shown: string;
  createdAtMs: number;
  lastUsedAtMs: number | null;
  /** The only agent this key speaks as; null for a general key. */
  agentId: string | null;
  /** P84 — the member it belongs to; null for yours. */
  user: string | null;
  /** P131 — the providers and combos (ids, or "*") the key may call straight with no agent; empty for none. */
  raw: string[];
}

const dateFormatter = () => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" });

/** Where the embedded hub answers the API, from its status. */
function apiBaseUrl(status: EmbeddedServerStatus | null): string | null {
  if (!status?.running) return null;
  if (status.secureUrl) return `${status.secureUrl.replace(/^wss:\/\//, "https://").replace(/\/$/, "")}/v1`;
  if (!status.boundAddr) return null;
  const port = status.boundAddr.split(":").pop();
  const host = status.boundAddr.startsWith("0.0.0.0") ? t("apiKeys.thisMachine") : status.boundAddr.replace(/:\d+$/, "");
  return `${status.secure ? "https" : "http"}://${host}:${port}/v1`;
}

/**
 * P12 — the Warden API: the embedded hub's agent behind the OpenAI chat-completions format, for
 * scripts, n8n or any client that talks to OpenAI. The same keys as the web's Settings and
 * `warden-server api-keys` (one file). Lives outside Settings' form: keys save on their own.
 */
function ApiKeysSection() {
  const [keys, setKeys] = useState<ApiKeyInfo[]>([]);
  const [status, setStatus] = useState<EmbeddedServerStatus | null>(null);
  const [name, setName] = useState("");
  /** "" = a general key. */
  const [agentId, setAgentId] = useState("");
  const [agentIds, setAgentIds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [created, setCreated] = useState<{ name: string; key: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [confirmRevoke, setConfirmRevoke] = useState<string | null>(null);
  /** P131: the providers and combos the config has, for the "Direct calls" editor. */
  const [options, setOptions] = useState<RawOption[]>([]);
  /** The key whose "Direct calls" is open for editing, with the ticks so far. */
  const [editing, setEditing] = useState<{ id: string; raw: string[] } | null>(null);

  function load() {
    invoke<ApiKeyInfo[]>("list_api_keys")
      .then(setKeys)
      .catch((err) => setError(String(err)));
    invoke<EmbeddedServerStatus>("embedded_server_status")
      .then(setStatus)
      .catch(() => setStatus(null));
    invoke<Settings>("get_settings")
      .then((settings) => {
        setAgentIds(settings.agents.map((a) => a.id));
        setOptions(rawOptions(settings.providers, settings.combos));
      })
      .catch(() => {
        setAgentIds([]);
        setOptions([]);
      });
  }

  async function handleSaveRaw(id: string, raw: string[]) {
    setError(null);
    try {
      setKeys(await invoke<ApiKeyInfo[]>("set_api_key_raw", { id, raw }));
      setEditing(null);
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(load, []);

  async function handleCreate() {
    setError(null);
    try {
      const result = await invoke<{ key: string; info: ApiKeyInfo }>("create_api_key", { name, agentId: agentId || null });
      setCreated({ name: result.info.name, key: result.key });
      setCopied(false);
      setName("");
      setAgentId("");
      load();
    } catch (err) {
      setError(String(err));
    }
  }

  async function handleRevoke(id: string) {
    setError(null);
    setConfirmRevoke(null);
    try {
      await invoke("revoke_api_key", { id });
      load();
    } catch (err) {
      setError(String(err));
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  const baseUrl = apiBaseUrl(status);

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("apiKeys.title")}</h3>
      </div>
      <p className="settings-hint">
        {rich("apiKeys.hint", {
          warden: <code>warden</code>,
          wardenAgent: <code>warden/&lt;agent&gt;</code>,
          api: <code>api</code>,
          provider: <code>provider/&lt;id&gt;</code>,
          combo: <code>combo/&lt;id&gt;</code>,
          apiRaw: <code>api-raw</code>,
        })}
      </p>
      <p className="settings-hint">{baseUrl ? rich("apiKeys.baseUrl", { url: <code>{baseUrl}</code> }) : t("apiKeys.turnOn")}</p>

      {error && <p className="settings-error-banner">{error}</p>}

      {created && (
        <div className="api-key-created">
          <p className="settings-hint">
            {rich("apiKeys.created", { name: <strong>{created.name}</strong> })}
          </p>
          <code className="api-key-value">{created.key}</code>
          <div className="api-key-actions">
            <button type="button" className="settings-browse-btn" onClick={() => void copy(created.key)}>
              {copied ? t("copy.copied") : t("copy.copy")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setCreated(null)}>
              {t("apiKeys.done")}
            </button>
          </div>
        </div>
      )}

      {keys.length === 0 ? (
        <p className="settings-hint">{t("apiKeys.none")}</p>
      ) : (
        <ul className="api-key-list">
          {keys.map((key) => (
            <li key={key.id} className="api-key-item">
              <div>
                <strong>{key.name}</strong> <code>{key.shown}…</code>
                <div className="settings-hint">
                  {key.user && t("apiKeys.userKey", { user: key.user })}
                  {key.agentId
                    ? `${t("apiKeys.onlyAs", { agent: key.agentId })}${agentIds.length > 0 && !agentIds.includes(key.agentId) ? t("apiKeys.agentRemoved") : ""}`
                    : t("apiKeys.general")}
                </div>
                {!canEditRaw(key) ? (
                  <div className="settings-hint">{t("apiKeys.rawOnlyGeneral")}</div>
                ) : editing?.id === key.id ? (
                  <fieldset className="settings-field">
                    <legend className="settings-hint">{t("apiKeys.rawLegend")}</legend>
                    {options.length === 0 && <span className="settings-hint">{t("apiKeys.rawNoProviders")}</span>}
                    <label>
                      <input type="checkbox" checked={hasAll(editing.raw)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, RAW_ALL) })} />{" "}
                      {rich("apiKeys.rawAll", { star: <code>*</code> })}
                    </label>
                    {options.map((o) => (
                      <label key={o.id}>
                        <input type="checkbox" checked={editing.raw.includes(o.id)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, o.id) })} /> {o.id}{" "}
                        <span className="settings-hint">{o.far ? t("apiKeys.rawFar", { kind: o.kind }) : o.kind}</span>
                      </label>
                    ))}
                    {missingIds(editing.raw, options).map((id) => (
                      <label key={id}>
                        <input type="checkbox" checked onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, id) })} /> {id}{" "}
                        <span className="settings-hint">{t("apiKeys.rawGone")}</span>
                      </label>
                    ))}
                    <div className="api-key-actions">
                      <button type="button" className="settings-browse-btn" onClick={() => void handleSaveRaw(key.id, editing.raw)}>
                        {t("common.save")}
                      </button>
                      <button type="button" className="settings-browse-btn" onClick={() => setEditing(null)}>
                        {t("common.cancel")}
                      </button>
                    </div>
                  </fieldset>
                ) : (
                  <div className="settings-hint">
                    {rich("apiKeys.rawLabel", { summary: <strong>{rawSummary(key.raw)}</strong> })}{" "}
                    <button type="button" className="settings-browse-btn" disabled={editing !== null} onClick={() => setEditing({ id: key.id, raw: key.raw })}>
                      {t("common.edit")}
                    </button>
                  </div>
                )}
                <div className="settings-hint">
                  {t("apiKeys.createdAt", { date: dateFormatter().format(new Date(key.createdAtMs)) })} ·{" "}
                  {key.lastUsedAtMs ? t("apiKeys.lastUsed", { date: dateFormatter().format(new Date(key.lastUsedAtMs)) }) : t("apiKeys.neverUsed")}
                </div>
              </div>
              {confirmRevoke === key.id ? (
                <div className="api-key-actions">
                  <button type="button" className="settings-browse-btn" onClick={() => void handleRevoke(key.id)}>
                    {t("apiKeys.revokeForGood")}
                  </button>
                  <button type="button" className="settings-browse-btn" onClick={() => setConfirmRevoke(null)}>
                    {t("common.keep")}
                  </button>
                </div>
              ) : (
                <button type="button" className="settings-browse-btn" onClick={() => setConfirmRevoke(key.id)}>
                  {t("apiKeys.revoke")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}

      <div className="settings-key-field">
        <input
          className="settings-input"
          type="text"
          placeholder={t("apiKeys.namePlaceholder")}
          maxLength={60}
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
        />
        <select className="settings-input" value={agentId} onChange={(e) => setAgentId(e.currentTarget.value)} aria-label={t("apiKeys.agentLabel")}>
          <option value="">{t("apiKeys.anyAgent")}</option>
          {agentIds.map((id) => (
            <option key={id} value={id}>
              {t("apiKeys.onlyAgent", { id })}
            </option>
          ))}
        </select>
        <button type="button" className="settings-browse-btn" disabled={name.trim() === ""} onClick={() => void handleCreate()}>
          {t("apiKeys.create")}
        </button>
      </div>
    </section>
  );
}

export default ApiKeysSection;
