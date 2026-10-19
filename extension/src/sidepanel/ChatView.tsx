import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatEntry } from "../background/connection";
import { PopupMarkdownLink } from "./MarkdownLink";
import type { LiveTurn } from "../protocol/liveTurn";
import { isLastAnswer, lastUserEntry, messagesAfter } from "../protocol/regenerate";
import { repliesLabel, type ThreadInfo } from "../protocol/threads";
import { t } from "../i18n/index.ts";

interface Props {
  serverName: string;
  history: ChatEntry[];
  pending: boolean;
  /** P124 — what the engine has done so far in the turn being waited on, shown in place of the dots. */
  live?: LiveTurn;
  /** P124 — stops the task that turn is running. Only given for a conversation that can have one. */
  onCancel?: () => void;
  /** P125 — the threads of the open conversation, by the id of the message they hang from. */
  threads: Record<string, ThreadInfo>;
  /** P125 — the open conversation is itself a thread: no thread inside a thread. */
  inThread: boolean;
  onOpenThread: (messageId: string) => void;
  /** P157 — regenerates the last answer or edits a message of the person's: the turn takes the place of the message `fromId` and of all that
   * followed. Absent where the conversation can't be redone (a thread, a code project, one not on the hub yet, an answer on its way). */
  onRedo?: (fromId: string, message: string) => void;
  onSend: (message: string) => void;
  onDisconnect: () => void;
}

/** P157 — a message of the person's turned into a box to change it. Sending asks first when the edit also drops what came after it. */
function EditBox({ initial, after, onSend, onCancel }: { initial: string; after: number; onSend: (message: string) => void; onCancel: () => void }) {
  const [text, setText] = useState(initial);

  function send() {
    const message = text.trim();
    if (message === "") return;
    if (after > 0 && !window.confirm(t("chat.confirmEditDeletes", { what: after === 1 ? t("chat.oneMessage") : t("chat.nMessages", { count: after }) }))) return;
    onSend(message);
  }

  return (
    <div className="message-edit">
      <textarea
        className="message-edit-input"
        value={text}
        autoFocus
        aria-label={t("chat.editLabel")}
        rows={Math.min(8, Math.max(2, text.split("\n").length))}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) send();
        }}
      />
      <div className="chat-entry-thread">
        <button type="button" className="link-button" onClick={onCancel}>
          {t("common.cancel")}
        </button>
        <button type="button" className="link-button" onClick={send}>
          {t("common.send")}
        </button>
      </div>
    </div>
  );
}

const TOOL_MARK = { running: "…", completed: "✓", failed: "✗" } as const;

/** P124 — what the engine is doing in the turn that is going on: its words and tools in the order they happened. */
function LiveBubble({ live, onCancel }: { live: LiveTurn; onCancel?: () => void }) {
  return (
    <li className="chat-entry chat-entry--assistant chat-entry--live" aria-label={t("chat.inProgress")}>
      {live.items.map((item, i) =>
        item.kind === "text" ? (
          <div key={i} className="chat-entry-content">
            {/* Words of a manager's task say which one, since several can be writing at once. */}
            {item.source && <p className="live-source">{item.source}</p>}
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: PopupMarkdownLink }}>
              {item.text}
            </ReactMarkdown>
          </div>
        ) : (
          <p key={i} className={`live-tool live-tool--${item.status}`}>
            <span className="live-tool-mark" aria-hidden="true">
              {TOOL_MARK[item.status]}
            </span>
            <span className="live-tool-name">{item.tool}</span>
            <span className="live-tool-title">{item.title}</span>
          </p>
        ),
      )}
      {live.notice && <p className="live-notice">{live.notice}</p>}
      {onCancel && (
        <button type="button" className="link-button live-stop" onClick={onCancel}>
          {t("chat.stop")}
        </button>
      )}
    </li>
  );
}

export default function ChatView({ serverName, history, pending, live, onCancel, threads, inThread, onOpenThread, onRedo, onSend, onDisconnect }: Props) {
  const [draft, setDraft] = useState("");
  // The message of the person's being edited (P157), by its id; one at a time.
  const [editingId, setEditingId] = useState<string | null>(null);
  const lastUser = lastUserEntry(history);

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = draft.trim();
    if (!trimmed || pending) return;
    onSend(trimmed);
    setDraft("");
  }

  return (
    <div className="chat-view">
      <header className="chat-header">
        <span>{t("chat.connectedTo", { server: serverName })}</span>
        <button type="button" className="link-button" onClick={onDisconnect}>
          {t("chat.disconnect")}
        </button>
      </header>

      <ul className="chat-history" aria-live="polite">
        {history.map((entry, i) => (
          <li key={i} className={`chat-entry chat-entry--${entry.role}`}>
            {editingId !== null && entry.id === editingId && onRedo ? (
              <EditBox
                initial={entry.content}
                after={messagesAfter(history, editingId)}
                onSend={(message) => {
                  setEditingId(null);
                  onRedo(editingId, message);
                }}
                onCancel={() => setEditingId(null)}
              />
            ) : (
              <div className="chat-entry-content">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: PopupMarkdownLink }}>
                  {entry.content}
                </ReactMarkdown>
              </div>
            )}
            {onRedo && entry.id && editingId !== entry.id && (entry.role === "user" || (lastUser && isLastAnswer(history, i))) && (
              <div className="chat-entry-thread">
                {entry.role === "user" ? (
                  <button type="button" className="link-button" onClick={() => setEditingId(entry.id!)}>
                    {t("chat.edit")}
                  </button>
                ) : (
                  <button type="button" className="link-button" onClick={() => onRedo(lastUser!.id!, lastUser!.content)}>
                    {t("chat.regenerate")}
                  </button>
                )}
              </div>
            )}
            {!inThread && entry.id && entry.role !== "error" && (
              // P125 — a message the hub already has an id for can start a thread, or show the one it has.
              <div className="chat-entry-thread">
                {threads[entry.id] && threads[entry.id].replies > 0 ? (
                  <button type="button" className="link-button" onClick={() => onOpenThread(entry.id!)}>
                    {repliesLabel(threads[entry.id].replies)}
                  </button>
                ) : (
                  <button type="button" className="link-button" onClick={() => onOpenThread(entry.id!)}>
                    {t("chat.replyInThread")}
                  </button>
                )}
              </div>
            )}
          </li>
        ))}
        {pending && live && <LiveBubble live={live} onCancel={onCancel} />}
        {pending && !live && (
          <li className="chat-entry chat-entry--pending">
            …
            {onCancel && (
              <button type="button" className="link-button live-stop" onClick={onCancel}>
                {t("chat.stop")}
              </button>
            )}
          </li>
        )}
      </ul>

      <form className="chat-composer" onSubmit={handleSubmit}>
        <input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={t("chat.message")} disabled={pending} />
        <button type="submit" disabled={pending || !draft.trim()}>
          {t("common.send")}
        </button>
      </form>
    </div>
  );
}
