import { useCallback, useEffect, useRef, useState } from "react";
import type { ChatEntry, ServerConnection } from "../hub/connection";
import { historyToEntries } from "../hub/connection";
import type { Attachment } from "../hub/messages";
import { t } from "../i18n";
import ChatView from "./ChatView";

const HISTORY_LIMIT = 200;

function errorText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** P125 — the thread of a message: a separate chat, to the right of the conversation, tied to a child conversation of it. The model sees the
 * conversation up to the message the thread came from and then only what is said here; nothing goes back to the main conversation, only the
 * reply counter on the message. `threadId` is the thread's conversation (the one that already exists, or a new id the hub creates with the
 * first reply), and `parent` is the message it comes from, sent only with the first one. */
export default function ThreadPanel({
  conn,
  threadId,
  parent,
  anchor,
  agentIds,
  initialAgentId,
  disabled,
  onClose,
}: {
  conn: ServerConnection;
  threadId: string;
  parent: { conversationId: string; messageId: string };
  /** The message the thread came from, for the top of the panel. */
  anchor: ChatEntry;
  /** The agents configured on the hub, to choose who the thread talks to. */
  agentIds: string[];
  /** Who the thread starts with: the agent it last talked to, or the main conversation's if it is new. The person changes it in the panel. */
  initialAgentId: string;
  disabled: boolean;
  onClose: () => void;
}) {
  const [agentId, setAgentId] = useState(initialAgentId);
  const [entries, setEntries] = useState<ChatEntry[]>([]);
  const [pending, setPending] = useState(false);
  const threadRef = useRef(threadId);
  threadRef.current = threadId;

  // The thread's history, if it already exists (one that does not exist yet comes back empty).
  useEffect(() => {
    let alive = true;
    setEntries([]);
    setPending(false);
    conn.fetchHistory(threadId, HISTORY_LIMIT).then(
      (history) => alive && setEntries(historyToEntries(history)),
      (err) => alive && setEntries([{ role: "error", content: t("thread.loadError", { error: errorText(err) }), attachments: [] }]),
    );
    return () => {
      alive = false;
    };
  }, [conn, threadId]);

  // The thread's reply (or error) arrives over the same connection as the main chat, tagged with its id.
  useEffect(
    () =>
      conn.onChatMessage((entry, conversationId) => {
        if (conversationId !== threadRef.current) return;
        setPending(false);
        setEntries((current) => [...current, entry]);
      }),
    [conn],
  );

  const send = useCallback(
    (message: string, attachments: Attachment[]) => {
      setEntries((current) => [...current, { role: "user", content: message, attachments }]);
      setPending(true);
      try {
        conn.sendChat(message, threadId, attachments, agentId || undefined, undefined, undefined, parent);
      } catch (err) {
        setPending(false);
        setEntries((current) => [...current, { role: "error", content: errorText(err), attachments: [] }]);
      }
    },
    [conn, threadId, agentId, parent],
  );

  return (
    <aside className="thread-panel" aria-label={t("thread.title")}>
      <div className="thread-header">
        <strong>{t("thread.title")}</strong>
        {(agentIds.length > 0 || agentId !== "") && (
          // Each thread talks to the agent the person picks, the same as the main conversation's or another; locked while the reply is pending.
          <label className="agent-picker">
            <span className="agent-picker-label">{t("picker.agent")}</span>
            <select value={agentId} onChange={(e) => setAgentId(e.target.value)} disabled={pending}>
              <option value="">{t("picker.none")}</option>
              {agentIds.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
              {agentId !== "" && !agentIds.includes(agentId) && <option value={agentId}>{t("picker.removed", { agent: agentId })}</option>}
            </select>
          </label>
        )}
        <button type="button" className="link-button" onClick={onClose} aria-label={t("thread.closeLabel")}>
          {t("thread.close")}
        </button>
      </div>
      <blockquote className="thread-anchor">
        <span className="skills-hint">{t("thread.said", { who: anchor.role === "user" ? t("thread.you") : t("thread.assistant") })}</span>
        <p>{anchor.content.length > 400 ? `${anchor.content.slice(0, 400)}…` : anchor.content}</p>
      </blockquote>
      <ChatView
        entries={entries}
        pending={pending}
        onCancel={() => undefined}
        disabled={disabled}
        onSend={send}
        onTranscribe={(audio) => conn.transcribe(audio)}
        onExtendLimit={(limitId) => conn.extendLimit(limitId).then(() => undefined)}
      />
    </aside>
  );
}
