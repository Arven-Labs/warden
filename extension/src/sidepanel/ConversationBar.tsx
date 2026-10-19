import { useState } from "react";
import { groupConversations } from "../protocol/conversationGroups";
import type { ConversationSummary, ThreadParent } from "../protocol/messages";
import { visibleConversations } from "../protocol/threads";
import type { OkResponse } from "../background/popup_protocol";
import { t } from "../i18n/index.ts";
import FolderPicker from "./FolderPicker";
import { folderLabel } from "./workdir";

interface Props {
  conversations: ConversationSummary[];
  activeConversationId: string | null;
  pendingIds: string[];
  agentIds: string[];
  agentId: string | null;
  /** P102 — the open conversation's working folder, or the one a new conversation will start in. */
  workdir: string | null;
  /** P125 — the app shows the thread bar instead of this one while a thread is open, so this is always null here. */
  threadParent?: ThreadParent | null;
}

/** P78 — which of this device's conversations the chat shows, plus new/rename/delete. A `<select>`
 * rather than a list: the side panel is narrow. Talks to the background directly, like `SkillsView`;
 * the result comes back as a `conversationsChanged` event. */
export default function ConversationBar({ conversations, activeConversationId, pendingIds, agentIds, agentId, workdir }: Props) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const [pickingFolder, setPickingFolder] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const active = conversations.find((c) => c.id === activeConversationId);
  // Nothing on the hub to rename or delete yet, or its answer is still on the way.
  const locked = busy || !active || pendingIds.includes(active.id);

  async function run(request: object) {
    setBusy(true);
    setError(null);
    const res = (await chrome.runtime.sendMessage(request)) as OkResponse;
    setBusy(false);
    if (!res.ok) setError(res.error ?? t("bar.failed"));
    return res.ok;
  }

  function select(conversationId: string) {
    setPickingFolder(false);
    setRenaming(null);
    setConfirmDelete(false);
    setError(null);
    void chrome.runtime.sendMessage({ type: "selectConversation", conversationId });
  }

  function startNew() {
    setPickingFolder(false);
    setRenaming(null);
    setConfirmDelete(false);
    setError(null);
    void chrome.runtime.sendMessage({ type: "newConversation" });
  }

  async function saveRename(e: React.FormEvent) {
    e.preventDefault();
    if (!active || renaming === null) return;
    const title = renaming.trim();
    if (title && title !== active.title && !(await run({ type: "renameConversation", conversationId: active.id, title }))) return;
    setRenaming(null);
  }

  async function remove() {
    if (!active) return;
    if (await run({ type: "deleteConversation", conversationId: active.id })) setConfirmDelete(false);
  }

  // P157 — pinned first, then by how recently they were used, with the archived ones at the end; a native group fits the narrow panel.
  const { pinned, buckets, archived } = groupConversations(visibleConversations(conversations), Date.now());
  const option = (c: ConversationSummary) => (
    <option key={c.id} value={c.id}>
      {pendingIds.includes(c.id) ? t("bar.answering", { title: c.title }) : c.title || t("bar.untitled")}
    </option>
  );

  if (renaming !== null) {
    return (
      <form className="conversation-bar" onSubmit={saveRename}>
        <input autoFocus value={renaming} maxLength={120} aria-label={t("bar.newTitle")} disabled={busy} onChange={(e) => setRenaming(e.target.value)} />
        <button type="submit" disabled={busy || !renaming.trim()}>
          {t("common.save")}
        </button>
        <button type="button" className="link-button" onClick={() => setRenaming(null)}>
          {t("common.cancel")}
        </button>
        {error && <p className="error-banner">{error}</p>}
      </form>
    );
  }

  return (
    <div className="conversation-bar">
      <select aria-label={t("bar.conversation")} value={active ? active.id : ""} onChange={(e) => select(e.target.value)}>
        {!active && <option value="">{t("bar.newOption")}</option>}
        {pinned.length > 0 && <optgroup label={t("bar.pinned")}>{pinned.map(option)}</optgroup>}
        {buckets.map(({ label, items }) => (
          <optgroup key={label} label={label}>
            {items.map(option)}
          </optgroup>
        ))}
        {archived.length > 0 && <optgroup label={t("bar.archived")}>{archived.map(option)}</optgroup>}
      </select>
      <span className="conversation-bar-actions">
        {confirmDelete ? (
          <>
            <button type="button" className="link-button skills-danger" disabled={locked} onClick={() => void remove()}>
              {t("bar.deleteForSure")}
            </button>
            <button type="button" className="link-button" onClick={() => setConfirmDelete(false)}>
              {t("common.keep")}
            </button>
          </>
        ) : (
          <>
            <button type="button" className="link-button" onClick={startNew}>
              {t("bar.new")}
            </button>
            {active && !active.archived && (
              <button type="button" className="link-button" disabled={locked} onClick={() => void run({ type: "setConversationFlags", conversationId: active.id, pinned: !active.pinned })}>
                {active.pinned ? t("bar.unpin") : t("bar.pin")}
              </button>
            )}
            {active && (
              <button type="button" className="link-button" disabled={locked} onClick={() => void run({ type: "setConversationFlags", conversationId: active.id, archived: !active.archived })}>
                {active.archived ? t("bar.unarchive") : t("bar.archive")}
              </button>
            )}
            <button type="button" className="link-button" disabled={locked} onClick={() => active && setRenaming(active.title)}>
              {t("bar.rename")}
            </button>
            <button type="button" className="link-button" disabled={locked} onClick={() => setConfirmDelete(true)}>
              {t("common.delete")}
            </button>
          </>
        )}
      </span>
      {(agentIds.length > 0 || agentId !== null) && (
        // P87 — which configured agent the next turns speak as; locked while an answer is on the way.
        <label className="conversation-bar-agent">
          {t("app.agent")}
          <select
            value={agentId ?? ""}
            disabled={activeConversationId !== null && pendingIds.includes(activeConversationId)}
            onFocus={() => void chrome.runtime.sendMessage({ type: "refreshAgents" })}
            onChange={(e) => void chrome.runtime.sendMessage({ type: "selectAgent", agentId: e.target.value || null })}
          >
            <option value="">{t("common.none")}</option>
            {agentIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
            {agentId !== null && !agentIds.includes(agentId) && <option value={agentId}>{t("common.removedAgent", { agent: agentId })}</option>}
          </select>
        </label>
      )}
      {/* P102 — the working folder: picked before the first message, only shown after it. */}
      {(active ? workdir !== null : true) && (
        <span className="conversation-bar-agent">
          {t("bar.folder")}
          {active ? (
            <span className="folder-chip" title={workdir ?? ""}>
              {folderLabel(workdir ?? "")}
            </span>
          ) : (
            <>
              <button type="button" className="link-button folder-chip" onClick={() => setPickingFolder(true)} title={workdir ?? t("bar.pickFolderTitle")}>
                {workdir !== null ? folderLabel(workdir) : t("bar.noFolder")}
              </button>
              {workdir !== null && (
                <button type="button" className="link-button" aria-label={t("bar.clearFolder")} onClick={() => void run({ type: "selectWorkdir", path: null })}>
                  ×
                </button>
              )}
            </>
          )}
        </span>
      )}
      {pickingFolder && !active && (
        <FolderPicker
          initialPath={workdir}
          onCancel={() => setPickingFolder(false)}
          onPick={(path) => {
            setPickingFolder(false);
            void run({ type: "selectWorkdir", path });
          }}
        />
      )}
      {error && <p className="error-banner">{error}</p>}
    </div>
  );
}
