import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";
import type { AgentEntry, RunMessage, Webhook, WebhookAuth, WebhookCreated, WebhookInfo, WebhookList } from "../types";
import { hubWebhooks } from "../lib/hub";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** Where the webhooks live: this computer's `config.toml` and credentials, or the hub in use. The hub's changes ask for
 * its pairing key. */
interface WebhookApi {
  list: () => Promise<WebhookList>;
  save: (originalId: string | null, webhook: Webhook) => Promise<WebhookList>;
  setEnabled: (id: string, enabled: boolean) => Promise<WebhookList>;
  remove: (id: string) => Promise<WebhookList>;
  revoke: (id: string) => Promise<WebhookList>;
  makeCredential: (id: string) => Promise<WebhookCreated>;
  /** `id` is the webhook's, `conversation` its conversation's (`task-hook-<id>`). */
  history: (id: string, conversation: string) => Promise<RunMessage[]>;
}

const localWebhooks: WebhookApi = {
  list: () => invoke<WebhookList>("list_webhooks"),
  save: (originalId, webhook) => invoke<WebhookList>("save_webhook", { originalId, webhook }),
  setEnabled: (id, enabled) => invoke<WebhookList>("set_webhook_enabled_cmd", { id, enabled }),
  remove: (id) => invoke<WebhookList>("delete_webhook", { id }),
  revoke: (id) => invoke<WebhookList>("revoke_webhook_credential", { id }),
  makeCredential: (id) => invoke<WebhookCreated>("create_webhook_credential", { id }),
  history: (id) => invoke<RunMessage[]>("webhook_history", { id }),
};

interface EditorState {
  originalId?: string;
  /** What the webhook wanted before this edit, to warn that changing it drops the credential. */
  originalAuth?: WebhookAuth;
  webhook: Webhook;
}

const dateFormatter = () => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" });

// Getters, so each read says the words of the language the person has now.
const authNames: Record<WebhookAuth, string> = {
  get token() {
    return t("webhooks.authToken");
  },
  get hmac() {
    return t("webhooks.authHmac");
  },
};
const credentialNames: Record<WebhookAuth, string> = {
  get token() {
    return t("webhooks.credToken");
  },
  get hmac() {
    return t("webhooks.credHmac");
  },
};

function editorFor(hook: WebhookInfo): EditorState {
  return { originalId: hook.id, originalAuth: hook.auth, webhook: { id: hook.id, agentId: hook.agentId, prompt: hook.prompt, enabled: hook.enabled, auth: hook.auth } };
}

function statusLine(hook: WebhookInfo): string {
  const parts: string[] = [];
  if (!hook.credential) parts.push(t("webhooks.noCredential"));
  else if (hook.credential !== hook.auth) parts.push(t("webhooks.wrongCredential", { have: credentialNames[hook.credential], want: authNames[hook.auth] }));
  else {
    parts.push(t("webhooks.credentialShown", { kind: credentialNames[hook.credential], shown: hook.shown ?? "" }));
    if (hook.createdAtMs) parts.push(t("webhooks.createdAt", { date: dateFormatter().format(hook.createdAtMs) }));
    parts.push(hook.lastUsedAtMs ? t("webhooks.lastUsed", { date: dateFormatter().format(hook.lastUsedAtMs) }) : t("webhooks.neverUsed"));
  }
  if (!hook.enabled) parts.push(t("webhooks.paused"));
  return parts.join(" · ");
}

/** A webhook's conversation on this machine, read-only: the last answer, and the rest on request. */
function WebhookHistory({ hook, refreshKey, api }: { hook: WebhookInfo; refreshKey: number; api: WebhookApi }) {
  const [messages, setMessages] = useState<RunMessage[] | null>(null);
  const [all, setAll] = useState(false);

  useEffect(() => {
    api
      .history(hook.id, hook.conversation)
      .then(setMessages)
      .catch(() => setMessages([]));
  }, [hook.id, hook.conversation, refreshKey, api]);

  if (messages === null) return <p className="settings-hint">{t("common.loading")}</p>;
  if (messages.length === 0) return <p className="settings-hint">{t("webhooks.noCalls")}</p>;
  const shown = all ? messages : messages.filter((m) => m.role === "assistant").slice(-1);
  return (
    <div className="task-history">
      {shown.map((m, i) => (
        <div key={i} className={`task-history-message task-history-message--${m.role}`}>
          <span className="settings-hint">
            {m.role === "user" ? t("webhooks.received") : t("tasks.answer")} · {dateFormatter().format(m.createdAt)}
          </span>
          <p className="task-history-content">{m.content}</p>
        </div>
      ))}
      {messages.length > 1 && (
        <button type="button" className="settings-browse-btn" onClick={() => setAll(!all)}>
          {all ? t("tasks.onlyLast") : t("tasks.wholeHistory", { count: messages.length })}
        </button>
      )}
    </div>
  );
}

/**
 * P105 — incoming webhooks: a prompt an agent runs when something calls `POST /hooks/<id>` on this computer's embedded
 * hub with the webhook's credential. The same list, create, edit, pause, remove, credential and revoke as
 * `warden-server webhooks` and the web's Webhooks tab, on this machine's `config.toml` and credentials file. A new
 * credential is shown once, here, and never again.
 */
function WebhooksView({ agents, remote = false, hubUrl }: { agents: AgentEntry[]; remote?: boolean; hubUrl?: string }) {
  const { askKey, dialog } = usePairingKey();
  const api = useMemo(() => (remote ? hubWebhooks(askKey, hubUrl) : localWebhooks), [remote, askKey, hubUrl]);
  const [list, setList] = useState<WebhookList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);
  const [created, setCreated] = useState<{ id: string; credential: string; kind: WebhookAuth } | null>(null);
  const [copied, setCopied] = useState(false);

  const load = useCallback(() => {
    api
      .list()
      .then((next) => {
        setList(next);
        setRefreshKey((k) => k + 1);
      })
      .catch((err) => setError(String(err)));
  }, [api]);

  useEffect(load, [load]);

  // Calls come from outside, so look again now and then: the last use and the last answer change by themselves.
  useEffect(() => {
    const timer = window.setInterval(load, 5000);
    return () => window.clearInterval(timer);
  }, [load]);

  /** Runs one change and shows the list it answers with. False when it failed, or when the person declined to give the key. */
  async function act(change: () => Promise<WebhookList>): Promise<boolean> {
    setError(null);
    try {
      setList(await change());
      setRefreshKey((k) => k + 1);
      return true;
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setError(String(err));
      return false;
    }
  }

  async function makeCredential(hook: WebhookInfo) {
    setError(null);
    try {
      const made = await api.makeCredential(hook.id);
      setList(made.list);
      setRefreshKey((k) => k + 1);
      setCreated({ id: made.id, credential: made.credential, kind: made.kind });
      setCopied(false);
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setError(String(err));
    }
  }

  async function handleSave() {
    if (!editor) return;
    setSaving(true);
    const saved = await act(() => api.save(editor.originalId ?? null, editor.webhook));
    setSaving(false);
    if (saved) setEditor(null);
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  function updateHook(patch: Partial<Webhook>) {
    setEditor((current) => (current ? { ...current, webhook: { ...current.webhook, ...patch } } : current));
  }

  /** Where a caller reaches a webhook: the embedded hub's address while it runs, else only the path. */
  function addressOf(id: string): string {
    return list?.hubUrl ? `${list.hubUrl}/hooks/${id}` : `/hooks/${id}`;
  }

  const dropsCredential = editor?.originalAuth !== undefined && editor.originalAuth !== editor.webhook.auth;

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("webhooks.title")}</h2>
      <p className="settings-hint">{t("webhooks.hint")}</p>
      {list && !list.hubRunning && <p className="settings-hint">{remote ? t("webhooks.hubNotTaking") : t("webhooks.localNotTaking")}</p>}
      {error && <p className="settings-error-banner">{error}</p>}

      {created && (
        <div className="api-key-created">
          <p className="settings-hint">
            {rich(created.kind === "token" ? "webhooks.createdToken" : "webhooks.createdSecret", { id: <strong>{created.id}</strong> })}
          </p>
          <code className="api-key-value">{created.credential}</code>
          {created.kind === "token" ? (
            <p className="settings-hint">
              {rich("webhooks.tokenUsage", {
                address: <code>{addressOf(created.id)}</code>,
                bearer: <code>Authorization: Bearer {"<token>"}</code>,
                header: <code>X-Warden-Token</code>,
                curl: <code className="api-key-value">{`curl -X POST -H "Authorization: Bearer ${created.credential}" --data-binary @body.json ${addressOf(created.id)}`}</code>,
              })}
            </p>
          ) : (
            <p className="settings-hint">
              {rich(remote ? "webhooks.secretUsageHub" : "webhooks.secretUsageLocal", {
                secret: <em>{t("webhooks.secretWord")}</em>,
                address: <code>{addressOf(created.id)}</code>,
                file: <code>webhook_tokens.json</code>,
              })}
            </p>
          )}
          <div className="api-key-actions">
            <button type="button" className="settings-browse-btn" onClick={() => void copy(created.credential)}>
              {copied ? t("copy.copied") : t("copy.copy")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setCreated(null)}>
              {t("webhooks.done")}
            </button>
          </div>
        </div>
      )}

      {editor && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{editor.originalId ? t("webhooks.editTitle", { id: editor.originalId }) : t("webhooks.newWebhook")}</span>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.name")}</span>
            <input className="settings-input" type="text" placeholder="build-failed" value={editor.webhook.id} onChange={(e) => updateHook({ id: e.currentTarget.value })} />
            <span className="settings-hint">{t("webhooks.nameHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.agent")}</span>
            <select className="settings-select" value={editor.webhook.agentId ?? ""} onChange={(e) => updateHook({ agentId: e.currentTarget.value || undefined })}>
              <option value="">{t("tasks.noAgent")}</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.id}
                </option>
              ))}
            </select>
            <span className="settings-hint">{t("webhooks.agentHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.prompt")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={6}
              placeholder={t("webhooks.promptPlaceholder")}
              value={editor.webhook.prompt}
              onChange={(e) => updateHook({ prompt: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("webhooks.promptHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("webhooks.how")}</span>
            <select className="settings-select" value={editor.webhook.auth} onChange={(e) => updateHook({ auth: e.currentTarget.value as WebhookAuth })}>
              <option value="token">{t("webhooks.howToken")}</option>
              <option value="hmac">{t("webhooks.howHmac")}</option>
            </select>
            <span className="settings-hint">{editor.webhook.auth === "token" ? t("webhooks.howTokenHint") : t("webhooks.howHmacHint")}</span>
          </label>
          {dropsCredential && <p className="settings-hint">{t("webhooks.dropsCredential")}</p>}

          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={() => void handleSave()} disabled={saving}>
              {saving ? t("common.saving") : t("webhooks.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setEditor(null)} disabled={saving}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("webhooks.yours")}</h3>
          <button type="button" className="settings-browse-btn" onClick={() => setEditor({ webhook: { id: "", prompt: "", enabled: true, auth: "token" } })}>
            {t("webhooks.add")}
          </button>
        </div>

        {list === null ? (
          <p className="settings-hint">{t("webhooks.loading")}</p>
        ) : list.webhooks.length === 0 ? (
          <p className="settings-hint">{t("webhooks.none")}</p>
        ) : (
          <div className="provider-list">
            {list.webhooks.map((hook) => (
              <div className="provider-card skill-card" key={hook.id}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{hook.id}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === hook.id ? (
                      <>
                        <span className="settings-hint">{t("webhooks.confirmDelete")}</span>
                        <button type="button" className="provider-delete-btn" onClick={() => void act(() => api.remove(hook.id)).then(() => setConfirmDelete(null))}>
                          {t("common.delete")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          {t("common.keep")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-browse-btn" onClick={() => setExpanded(expanded === hook.id ? null : hook.id)}>
                          {expanded === hook.id ? t("tasks.hideResult") : t("tasks.lastResult")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => void makeCredential(hook)} title={hook.credential ? t("webhooks.replaceTitle") : undefined}>
                          {hook.credential === hook.auth
                            ? hook.auth === "token"
                              ? t("webhooks.replaceToken")
                              : t("webhooks.replaceSecret")
                            : hook.auth === "token"
                              ? t("webhooks.makeToken")
                              : t("webhooks.makeSecret")}
                        </button>
                        {hook.credential && (
                          <button type="button" className="settings-browse-btn" onClick={() => void act(() => api.revoke(hook.id))}>
                            {t("apiKeys.revoke")}
                          </button>
                        )}
                        <button type="button" className="settings-browse-btn" onClick={() => void act(() => api.setEnabled(hook.id, !hook.enabled))}>
                          {hook.enabled ? t("tasks.pause") : t("tasks.resume")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setEditor(editorFor(hook))}>
                          {t("common.edit")}
                        </button>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => setConfirmDelete(hook.id)}
                          aria-label={t("webhooks.deleteLabel", { id: hook.id })}
                          title={t("webhooks.deleteTitle")}
                        >
                          🗑
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">
                  POST {addressOf(hook.id)} · {authNames[hook.auth]} · {hook.agentId ? t("tasks.withAgent", { id: hook.agentId }) : t("tasks.withoutAgent")}
                </p>
                <p className="settings-hint">{statusLine(hook)}</p>
                {expanded === hook.id && <WebhookHistory hook={hook} refreshKey={refreshKey} api={api} />}
              </div>
            ))}
          </div>
        )}
      </section>
      {dialog}
    </div>
  );
}

export default WebhooksView;
