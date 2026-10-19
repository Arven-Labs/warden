import { useCallback, useEffect, useState } from "react";
import { WebhookError, type ServerConnection } from "../hub/connection";
import type { Webhook, WebhookAuth, WebhookInfo } from "../hub/messages";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";

// Incoming webhooks (P105): a prompt an agent runs when something calls `POST /hooks/<id>` on this hub with the
// webhook's credential. The same list, create, edit, pause, remove, credential and revoke as `warden-server webhooks`.
// Every change asks for the pairing key, like the tasks and the API keys: a webhook lets a stranger's call run an agent,
// with its tools and spend. A new credential is shown once, here, and never again. Each call lands in the webhook's
// conversation (`task-hook-<id>`), which "Open conversation" opens in the chat.

interface EditorState {
  /** The id being edited; absent for a new webhook. */
  originalId?: string;
  /** What the webhook wanted before this edit, to warn that changing it drops the credential. */
  originalAuth?: WebhookAuth;
  webhook: Webhook;
}

type Change =
  | { kind: "save"; editor: EditorState }
  | { kind: "toggle"; hook: WebhookInfo }
  | { kind: "delete"; hook: WebhookInfo }
  | { kind: "credential"; hook: WebhookInfo }
  | { kind: "revoke"; hook: WebhookInfo };

/** A credential just made. */
interface Created {
  id: string;
  credential: string;
  kind: WebhookAuth;
}

const dateFormatter = { format: (ms: number) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(ms) };

const authName = (auth: WebhookAuth) => t(auth === "token" ? "hooks.authToken" : "hooks.authHmac");
const credentialName = (auth: WebhookAuth) => t(auth === "token" ? "hooks.credToken" : "hooks.credHmac");

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function newEditor(): EditorState {
  return { webhook: { id: "", prompt: "", enabled: true, auth: "token" } };
}

function editorFor(hook: WebhookInfo): EditorState {
  return { originalId: hook.id, originalAuth: hook.auth, webhook: { id: hook.id, agentId: hook.agentId, prompt: hook.prompt, enabled: hook.enabled, auth: hook.auth } };
}

/** Where the hub is called, from the address this page was opened at. */
function hookUrl(id: string): string {
  return `${window.location.origin}/hooks/${id}`;
}

function status(hook: WebhookInfo): string {
  const parts: string[] = [];
  if (!hook.credential) parts.push(t("hooks.noCredential"));
  else if (hook.credential !== hook.auth) parts.push(t("hooks.credentialMismatch", { have: credentialName(hook.credential), want: authName(hook.auth) }));
  else {
    parts.push(`${credentialName(hook.credential)} ${hook.shown ?? ""}…`);
    if (hook.createdAtMs) parts.push(t("hooks.createdAt", { when: dateFormatter.format(hook.createdAtMs) }));
    parts.push(hook.lastUsedAtMs ? t("hooks.lastUse", { when: dateFormatter.format(hook.lastUsedAtMs) }) : t("hooks.neverUsed"));
  }
  if (!hook.enabled) parts.push(t("hooks.paused"));
  return parts.join(" · ");
}

export default function WebhooksView({ conn, onOpenConversation }: { conn: ServerConnection | null; onOpenConversation: (id: string) => void }) {
  const [hooks, setHooks] = useState<WebhookInfo[] | null>(null);
  const [servesHere, setServesHere] = useState(true);
  const [agentIds, setAgentIds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [asking, setAsking] = useState<Change | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [created, setCreated] = useState<Created | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(async () => {
    if (!conn) return;
    try {
      const list = await conn.listWebhooks();
      setHooks(list.webhooks);
      setServesHere(list.servesHere);
      setError(null);
    } catch (err) {
      setHooks((current) => current ?? []);
      setError(t("hooks.listFailed", { error: message(err) }));
    }
  }, [conn]);

  useEffect(() => {
    void refresh();
    if (!conn) return;
    conn
      .requestSettings()
      .then(({ settings }) => setAgentIds(settings.agents.map((a) => a.id)))
      .catch(() => setAgentIds([]));
    // A call finished: when the credential was last used changed.
    return conn.onConversationsChanged((id) => {
      if (id.startsWith("task-hook-")) void refresh();
    });
  }, [conn, refresh]);

  function cancelKey() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  async function confirm() {
    if (!conn || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      if (asking.kind === "credential") {
        const made = await conn.createWebhookCredential(pairingKey, asking.hook.id);
        setHooks(made.webhooks);
        setServesHere(made.servesHere);
        setCreated({ id: made.id, credential: made.credential, kind: made.kind });
        setCopied(false);
      } else {
        const list =
          asking.kind === "save"
            ? await conn.saveWebhook(pairingKey, asking.editor.webhook, asking.editor.originalId)
            : asking.kind === "toggle"
              ? await conn.setWebhookEnabled(pairingKey, asking.hook.id, !asking.hook.enabled)
              : asking.kind === "delete"
                ? await conn.deleteWebhook(pairingKey, asking.hook.id)
                : await conn.revokeWebhookCredential(pairingKey, asking.hook.id);
        setHooks(list.webhooks);
        setServesHere(list.servesHere);
        if (asking.kind === "save") setEditor(null);
        // What was shown once is only worth keeping on screen while it can still be copied from the right webhook.
        if (asking.kind === "delete" || asking.kind === "revoke") setCreated((current) => (current && current.id === asking.hook.id ? null : current));
      }
      setConfirmDelete(null);
      setError(null);
      cancelKey();
    } catch (err) {
      if (err instanceof WebhookError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setError(message(err));
      }
    } finally {
      setBusy(false);
    }
  }

  function ask(change: Change) {
    setError(null);
    setKeyError(null);
    setAsking(change);
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  const labels: Record<Change["kind"], string> = {
    save: t("hooks.saveWebhook"),
    toggle: t("common.confirm"),
    delete: t("common.delete"),
    credential: t("hooks.generate"),
    revoke: t("hooks.revoke"),
  };
  const keyPrompt = asking && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      {asking.kind === "credential" && asking.hook.credential && <p className="skills-hint">{t("hooks.credentialExists")}</p>}
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("pairingKey.hint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : labels[asking.kind]}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  function updateHook(patch: Partial<Webhook>) {
    setEditor((current) => (current ? { ...current, webhook: { ...current.webhook, ...patch } } : current));
  }

  if (editor) {
    const dropsCredential = editor.originalAuth !== undefined && editor.originalAuth !== editor.webhook.auth;
    return (
      <form
        className="skills-editor"
        onSubmit={(e) => {
          e.preventDefault();
          ask({ kind: "save", editor });
        }}
      >
        <strong>{editor.originalId ? t("hooks.editing", { id: editor.originalId }) : t("hooks.new")}</strong>
        <label>
          {t("common.name")}
          <input value={editor.webhook.id} onChange={(e) => updateHook({ id: e.target.value })} placeholder={t("hooks.namePlaceholder")} required />
          <span className="skills-hint">{t("hooks.nameHint")}</span>
        </label>
        <label>
          {t("hooks.agent")}
          <select value={editor.webhook.agentId ?? ""} onChange={(e) => updateHook({ agentId: e.target.value || undefined })}>
            <option value="">{t("hooks.noAgent")}</option>
            {agentIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
          <span className="skills-hint">{t("hooks.agentHint")}</span>
        </label>
        <label>
          {t("hooks.prompt")}
          <textarea
            value={editor.webhook.prompt}
            onChange={(e) => updateHook({ prompt: e.target.value })}
            rows={6}
            placeholder={t("hooks.promptPlaceholder")}
            required
          />
          <span className="skills-hint">{t("hooks.promptHint")}</span>
        </label>
        <label>
          {t("hooks.identity")}
          <select value={editor.webhook.auth} onChange={(e) => updateHook({ auth: e.target.value as WebhookAuth })}>
            <option value="token">{t("hooks.optToken")}</option>
            <option value="hmac">{t("hooks.optHmac")}</option>
          </select>
          <span className="skills-hint">{editor.webhook.auth === "token" ? t("hooks.hintToken") : t("hooks.hintHmac")}</span>
        </label>
        {dropsCredential && <p className="skills-hint">{t("hooks.dropsCredential")}</p>}
        {error && <p className="error-banner">{error}</p>}
        {asking ? (
          keyPrompt
        ) : (
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={!conn}>
              {t("common.save")}
            </button>
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setEditor(null);
                setError(null);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        )}
      </form>
    );
  }

  const createdHook = created && hooks?.find((h) => h.id === created.id);

  return (
    <div className="skills-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("hooks.intro")}</span>
        <button
          type="button"
          className="primary-button"
          onClick={() => {
            setError(null);
            setEditor(newEditor());
          }}
        >
          {t("hooks.newButton")}
        </button>
      </div>
      {!servesHere && <p className="skills-hint">{t("hooks.notServing")}</p>}
      {error && <p className="error-banner">{error}</p>}
      {asking && keyPrompt}

      {created && (
        <div className="settings-card api-key-created">
          <p className="skills-item-description">
            {rich(created.kind === "token" ? "hooks.createdToken" : "hooks.createdSecret", { id: <strong>{created.id}</strong> })}
          </p>
          <p>
            <code className="api-key-value">{created.credential}</code>
          </p>
          {created.kind === "token" ? (
            <p className="skills-hint">
              {rich("hooks.tokenUsage", {
                url: <code>{hookUrl(created.id)}</code>,
                auth: <code>Authorization: Bearer {"<token>"}</code>,
                alt: <code>X-Warden-Token</code>,
                example: <code className="api-key-value">{`curl -X POST -H "Authorization: Bearer ${created.credential}" --data-binary @body.json ${hookUrl(created.id)}`}</code>,
              })}
            </p>
          ) : (
            <p className="skills-hint">
              {rich("hooks.hmacUsage", {
                secret: <em>Secret</em>,
                url: <code>{hookUrl(created.id)}</code>,
                file: <code>webhook_tokens.json</code>,
              })}
            </p>
          )}
          {createdHook && !createdHook.enabled && <p className="skills-hint">{t("hooks.pausedNote")}</p>}
          <div className="skills-actions">
            <button type="button" className="primary-button" onClick={() => void copy(created.credential)}>
              {copied ? t("copy.copied") : t("copy.copy")}
            </button>
            <button type="button" className="link-button" onClick={() => setCreated(null)}>
              {t("apiKeys.done")}
            </button>
          </div>
        </div>
      )}

      {hooks === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : hooks.length === 0 ? (
        <p className="skills-hint">{t("hooks.none")}</p>
      ) : (
        <ul className="skills-list">
          {hooks.map((hook) => (
            <li key={hook.id} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{hook.id}</span>
                {confirmDelete === hook.id ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => ask({ kind: "delete", hook })}>
                      {t("common.delete")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      {t("common.keep")}
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button type="button" className="link-button" onClick={() => onOpenConversation(hook.conversation)}>
                      {t("hooks.openConversation")}
                    </button>
                    <button type="button" className="link-button" onClick={() => ask({ kind: "credential", hook })}>
                      {t(
                        hook.credential === hook.auth
                          ? hook.auth === "token"
                            ? "hooks.replaceToken"
                            : "hooks.replaceSecret"
                          : hook.auth === "token"
                            ? "hooks.generateToken"
                            : "hooks.generateSecret",
                      )}
                    </button>
                    {hook.credential && (
                      <button type="button" className="link-button" onClick={() => ask({ kind: "revoke", hook })}>
                        {t("hooks.revoke")}
                      </button>
                    )}
                    <button type="button" className="link-button" onClick={() => ask({ kind: "toggle", hook })}>
                      {hook.enabled ? t("hooks.pause") : t("hooks.resume")}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor(editorFor(hook));
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(hook.id)}>
                      {t("common.delete")}
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">
                POST {hookUrl(hook.id)} · {authName(hook.auth)} · {hook.agentId ? t("hooks.withAgent", { agent: hook.agentId }) : t("hooks.withoutAgent")}
              </p>
              <p className="skills-hint">{status(hook)}</p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
