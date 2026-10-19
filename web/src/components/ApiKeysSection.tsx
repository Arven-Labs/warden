import { useCallback, useEffect, useState } from "react";
import { ApiKeyError, type ServerConnection } from "../hub/connection";
import type { ApiKey } from "../hub/messages";
import { canEditRaw, hasAll, missingIds, RAW_ALL, rawOptions, rawSummary, toggleRaw, type RawOption } from "../hub/rawAccess";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";

// The Warden API (P12): this hub's agent behind the OpenAI chat-completions format, on the same
// address as this page, for scripts, n8n or any client that talks to OpenAI. The same list,
// create and revoke as the desktop's Settings and `warden-server api-keys`. Creating or revoking
// asks for the pairing key each time; a new key is shown once, here, and never again.

const dateFormatter = { format: (date: Date) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(date) };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

type Asking = { kind: "create"; name: string; agentId: string } | { kind: "revoke"; key: ApiKey } | { kind: "raw"; key: ApiKey; raw: string[] };

/** `member` (P84): the keys are this member's own, and changes are confirmed with their password. */
export default function ApiKeysSection({ conn, member = false }: { conn: ServerConnection | null; member?: boolean }) {
  const [keys, setKeys] = useState<ApiKey[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState("");
  /** "" = a general key. */
  const [agentId, setAgentId] = useState("");
  const [agentIds, setAgentIds] = useState<string[]>([]);
  /** P131: the providers and combos the hub has, for the "Direct calls" editor. */
  const [options, setOptions] = useState<RawOption[]>([]);
  /** The key whose "Direct calls" is open for editing, with the ticks so far. */
  const [editing, setEditing] = useState<{ id: string; raw: string[] } | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<{ name: string; key: string } | null>(null);
  const [copied, setCopied] = useState(false);

  const baseUrl = `${window.location.origin}/v1`;

  const load = useCallback(async () => {
    if (!conn) return;
    setError(null);
    try {
      setKeys(await conn.listApiKeys());
    } catch (err) {
      setError(message(err));
    }
    try {
      const { settings } = await conn.requestSettings();
      setAgentIds(settings.agents.map((a) => a.id));
      setOptions(rawOptions(settings.providers, settings.combos));
    } catch {
      setAgentIds([]);
      setOptions([]);
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
      if (asking.kind === "create") {
        const result = await conn.createApiKey(pairingKey, asking.name, asking.agentId || undefined);
        setKeys(result.keys);
        setCreated({ name: asking.name.trim(), key: result.key });
        setCopied(false);
        setName("");
        setAgentId("");
      } else if (asking.kind === "raw") {
        setKeys(await conn.setApiKeyRaw(pairingKey, asking.key.id, asking.raw));
        setEditing(null);
      } else {
        setKeys(await conn.revokeApiKey(pairingKey, asking.key.id));
      }
      cancel();
    } catch (err) {
      if (err instanceof ApiKeyError && err.authRejected) {
        setKeyError(member ? t("apiKeys.wrongPassword") : t("pairingKey.wrong"));
      } else {
        cancel();
        setError(message(err));
      }
    } finally {
      setBusy(false);
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

  const keyPrompt = asking && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      <label className="settings-field">
        {member ? t("apiKeys.yourPassword") : t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{member ? t("apiKeys.passwordHint") : t("pairingKey.hint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : asking.kind === "create" ? t("apiKeys.createKey") : asking.kind === "raw" ? t("common.save") : t("apiKeys.revoke")}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancel}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  return (
    <section className="usage-section settings-section">
      <div className="settings-section-header">
        <h2 className="usage-heading">{t("apiKeys.title")}</h2>
        <button type="button" className="link-button" onClick={() => void load()} disabled={!conn || busy}>
          {t("devices.reload")}
        </button>
      </div>
      <p className="skills-hint">
        {rich("apiKeys.intro", {
          base: <code>{baseUrl}</code>,
          model: <code>warden</code>,
          agentModel: <code>warden/&lt;agent&gt;</code>,
          apiChannel: <code>api</code>,
          prov: <code>provider/&lt;id&gt;</code>,
          combo: <code>combo/&lt;id&gt;</code>,
          rawChannel: <code>api-raw</code>,
        })}
        {member && t("apiKeys.memberNote")}
      </p>

      {error && <p className="error-banner">{error}</p>}

      {created && (
        <div className="settings-card api-key-created">
          <p className="skills-item-description">{rich("apiKeys.created", { name: <strong>{created.name}</strong> })}</p>
          <p>
            <code className="api-key-value">{created.key}</code>
          </p>
          <div className="skills-actions">
            <button type="button" className="primary-button" onClick={() => void copy(created.key)}>
              {copied ? t("apiKeys.copied") : t("apiKeys.copy")}
            </button>
            <button type="button" className="link-button" onClick={() => setCreated(null)}>
              {t("apiKeys.done")}
            </button>
          </div>
        </div>
      )}

      {keys && keys.length === 0 && <p className="skills-hint">{t("apiKeys.none")}</p>}
      {keys && keys.length > 0 && (
        <ul className="skills-list">
          {keys.map((key) => (
            <li key={key.id} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{key.name}</span>
                <code>{key.shown}…</code>
              </div>
              <p className="skills-item-description">
                {key.user && !member && <>{rich("apiKeys.from", { user: <strong>{key.user}</strong> })}</>}
                {key.agentId ? (
                  <>
                    {rich("apiKeys.onlyAs", { agent: <strong>{key.agentId}</strong> })}
                    {agentIds.length > 0 && !agentIds.includes(key.agentId) && t("apiKeys.agentRemoved")}
                  </>
                ) : (
                  t("apiKeys.general")
                )}
              </p>
              {!member && (
                <div className="skills-item-description">
                  {!canEditRaw(key, member) ? (
                    t("apiKeys.rawOnlyOwner")
                  ) : editing?.id === key.id ? (
                    <fieldset className="settings-field" disabled={asking !== null}>
                      <legend>{t("apiKeys.rawLegend")}</legend>
                      {options.length === 0 && <span className="field-hint">{t("apiKeys.noProviders")}</span>}
                      <label>
                        <input type="checkbox" checked={hasAll(editing.raw)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, RAW_ALL) })} />{" "}
                        {rich("apiKeys.allLocal", { star: <code>*</code> })}
                      </label>
                      {options.map((o) => (
                        <label key={o.id}>
                          <input type="checkbox" checked={editing.raw.includes(o.id)} onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, o.id) })} /> {o.id}{" "}
                          <span className="field-hint">
                            {o.kind === "combo" ? t("apiKeys.combo") : t("apiKeys.provider")}
                            {o.far && t("apiKeys.farNote")}
                          </span>
                        </label>
                      ))}
                      {missingIds(editing.raw, options).map((id) => (
                        <label key={id}>
                          <input type="checkbox" checked onChange={() => setEditing({ ...editing, raw: toggleRaw(editing.raw, id) })} /> {id}{" "}
                          <span className="field-hint">{t("apiKeys.gone")}</span>
                        </label>
                      ))}
                      <div className="skills-actions">
                        <button type="button" className="primary-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "raw", key, raw: editing.raw })}>
                          {t("apiKeys.saveEllipsis")}
                        </button>
                        <button type="button" className="link-button" disabled={asking !== null} onClick={() => setEditing(null)}>
                          {t("common.cancel")}
                        </button>
                      </div>
                    </fieldset>
                  ) : (
                    <>
                      {rich("apiKeys.rawLabel", { summary: <strong>{rawSummary(key.raw)}</strong> })}{" "}
                      <button type="button" className="link-button" disabled={asking !== null || editing !== null} onClick={() => setEditing({ id: key.id, raw: key.raw ?? [] })}>
                        {t("common.edit")}
                      </button>
                    </>
                  )}
                </div>
              )}
              {asking?.kind === "raw" && asking.key.id === key.id && keyPrompt}
              <p className="skills-item-description">
                {t("apiKeys.createdAt", {
                  when: dateFormatter.format(new Date(key.createdAtMs)),
                  used: key.lastUsedAtMs ? t("apiKeys.lastUsed", { when: dateFormatter.format(new Date(key.lastUsedAtMs)) }) : t("apiKeys.neverUsed"),
                })}
              </p>
              {asking?.kind === "revoke" && asking.key.id === key.id ? (
                keyPrompt
              ) : (
                <div className="skills-actions">
                  <button type="button" className="link-button skills-danger" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "revoke", key })}>
                    {t("apiKeys.revoke")}
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}

      {asking?.kind === "create" ? (
        keyPrompt
      ) : (
        <form
          className="skills-actions"
          onSubmit={(e) => {
            e.preventDefault();
            setAsking({ kind: "create", name, agentId });
          }}
        >
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder={t("apiKeys.namePlaceholder")} maxLength={60} aria-label={t("apiKeys.nameLabel")} />
          <select value={agentId} onChange={(e) => setAgentId(e.target.value)} aria-label={t("apiKeys.agentLabel")}>
            <option value="">{t("apiKeys.anyAgent")}</option>
            {agentIds.map((id) => (
              <option key={id} value={id}>
                {t("apiKeys.onlyAgent", { id })}
              </option>
            ))}
          </select>
          <button type="submit" className="primary-button" disabled={!conn || asking !== null || name.trim() === ""}>
            {t("apiKeys.create")}
          </button>
        </form>
      )}
    </section>
  );
}
