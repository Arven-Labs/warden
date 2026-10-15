import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

/** Where the embedded hub answers the API, from its status. */
function apiBaseUrl(status: EmbeddedServerStatus | null): string | null {
  if (!status?.running) return null;
  if (status.secureUrl) return `${status.secureUrl.replace(/^wss:\/\//, "https://").replace(/\/$/, "")}/v1`;
  if (!status.boundAddr) return null;
  const port = status.boundAddr.split(":").pop();
  const host = status.boundAddr.startsWith("0.0.0.0") ? "<this machine's address>" : status.boundAddr.replace(/:\d+$/, "");
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
        <h3 className="settings-section-title">Warden API</h3>
      </div>
      <p className="settings-hint">
        Your agent in the OpenAI chat-completions format, for scripts, n8n or any client that talks to OpenAI: use one of
        these keys as the API key. A general key picks the model per call (<code>warden</code> or{" "}
        <code>warden/&lt;agent&gt;</code>); a key bound to an agent only speaks as it. Tools the client sends are ignored,
        nothing is saved as a conversation, and the spend goes to the <code>api</code> channel. A general key can also call a
        provider or combo straight (<code>provider/&lt;id&gt;</code> or <code>combo/&lt;id&gt;</code>), with no agent, vault or
        tools: what it may call is chosen under "Direct calls", and the spend goes to the <code>api-raw</code> channel.
      </p>
      <p className="settings-hint">
        {baseUrl ? (
          <>
            Base URL: <code>{baseUrl}</code>
          </>
        ) : (
          "The API answers on the embedded hub's port — turn the hub on in Workspace to use it."
        )}
      </p>

      {error && <p className="settings-error-banner">{error}</p>}

      {created && (
        <div className="api-key-created">
          <p className="settings-hint">
            Key <strong>{created.name}</strong> created. Copy it now — it isn't shown again.
          </p>
          <code className="api-key-value">{created.key}</code>
          <div className="api-key-actions">
            <button type="button" className="settings-browse-btn" onClick={() => void copy(created.key)}>
              {copied ? "Copied" : "Copy"}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setCreated(null)}>
              Done
            </button>
          </div>
        </div>
      )}

      {keys.length === 0 ? (
        <p className="settings-hint">No keys yet.</p>
      ) : (
        <ul className="api-key-list">
          {keys.map((key) => (
            <li key={key.id} className="api-key-item">
              <div>
                <strong>{key.name}</strong> <code>{key.shown}…</code>
                <div className="settings-hint">
                  {key.user && `${key.user}'s key · `}
                  {key.agentId
                    ? `Only speaks as ${key.agentId}${agentIds.length > 0 && !agentIds.includes(key.agentId) ? " (agent removed — this key no longer works)" : ""}`
                    : "General — picks the agent per call"}
                </div>
                {!canEditRaw(key) ? (
                  <div className="settings-hint">Direct calls: only a general key of yours can.</div>
                ) : editing?.id === key.id ? (
                  <fieldset className="settings-field">
                    <legend className="settings-hint">Direct calls: what this key may call without going through an agent</legend>
                    {options.length === 0 && <span className="settings-hint">No providers are configured.</span>}
                    <label>
                      <input type="checkbox" checked={hasAll(editing.raw)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, RAW_ALL) })} /> All local
                      models (<code>*</code>)
                    </label>
                    {options.map((o) => (
                      <label key={o.id}>
                        <input type="checkbox" checked={editing.raw.includes(o.id)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, o.id) })} /> {o.id}{" "}
                        <span className="settings-hint">
                          {o.kind}
                          {o.far && " · on another machine: * doesn't cover it, it only counts ticked here"}
                        </span>
                      </label>
                    ))}
                    {missingIds(editing.raw, options).map((id) => (
                      <label key={id}>
                        <input type="checkbox" checked onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, id) })} /> {id}{" "}
                        <span className="settings-hint">no longer in the config: untick to drop it</span>
                      </label>
                    ))}
                    <div className="api-key-actions">
                      <button type="button" className="settings-browse-btn" onClick={() => void handleSaveRaw(key.id, editing.raw)}>
                        Save
                      </button>
                      <button type="button" className="settings-browse-btn" onClick={() => setEditing(null)}>
                        Cancel
                      </button>
                    </div>
                  </fieldset>
                ) : (
                  <div className="settings-hint">
                    Direct calls: <strong>{rawSummary(key.raw)}</strong>{" "}
                    <button type="button" className="settings-browse-btn" disabled={editing !== null} onClick={() => setEditing({ id: key.id, raw: key.raw })}>
                      Edit
                    </button>
                  </div>
                )}
                <div className="settings-hint">
                  Created {dateFormatter.format(new Date(key.createdAtMs))} ·{" "}
                  {key.lastUsedAtMs ? `last used ${dateFormatter.format(new Date(key.lastUsedAtMs))}` : "never used"}
                </div>
              </div>
              {confirmRevoke === key.id ? (
                <div className="api-key-actions">
                  <button type="button" className="settings-browse-btn" onClick={() => void handleRevoke(key.id)}>
                    Revoke for good
                  </button>
                  <button type="button" className="settings-browse-btn" onClick={() => setConfirmRevoke(null)}>
                    Keep
                  </button>
                </div>
              ) : (
                <button type="button" className="settings-browse-btn" onClick={() => setConfirmRevoke(key.id)}>
                  Revoke
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
          placeholder="Key name (e.g. n8n)"
          maxLength={60}
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
        />
        <select className="settings-input" value={agentId} onChange={(e) => setAgentId(e.currentTarget.value)} aria-label="Key's agent">
          <option value="">General (any agent)</option>
          {agentIds.map((id) => (
            <option key={id} value={id}>
              Only agent {id}
            </option>
          ))}
        </select>
        <button type="button" className="settings-browse-btn" disabled={name.trim() === ""} onClick={() => void handleCreate()}>
          Create key
        </button>
      </div>
    </section>
  );
}

export default ApiKeysSection;
