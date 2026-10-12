import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ChatEntry } from "../background/connection";
import { PopupMarkdownLink } from "./MarkdownLink";
import type { LiveTurn } from "../protocol/liveTurn";
import { repliesLabel, type ThreadInfo } from "../protocol/threads";

interface Props {
  serverName: string;
  history: ChatEntry[];
  pending: boolean;
  /** P124 — what the engine has done so far in the turn being waited on, shown in place of the dots. */
  live?: LiveTurn;
  /** P125 — the threads of the open conversation, by the id of the message they hang from. */
  threads: Record<string, ThreadInfo>;
  /** P125 — the open conversation is itself a thread: no thread inside a thread. */
  inThread: boolean;
  onOpenThread: (messageId: string) => void;
  onSend: (message: string) => void;
  onDisconnect: () => void;
}

const TOOL_MARK = { running: "…", completed: "✓", failed: "✗" } as const;

/** P124 — what the engine is doing in the turn that is going on: its words and tools in the order they happened. */
function LiveBubble({ live }: { live: LiveTurn }) {
  return (
    <li className="chat-entry chat-entry--assistant chat-entry--live" aria-label="Em andamento">
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
    </li>
  );
}

export default function ChatView({ serverName, history, pending, live, threads, inThread, onOpenThread, onSend, onDisconnect }: Props) {
  const [draft, setDraft] = useState("");

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
        <span>Conectado a {serverName}</span>
        <button type="button" className="link-button" onClick={onDisconnect}>
          Desconectar
        </button>
      </header>

      <ul className="chat-history" aria-live="polite">
        {history.map((entry, i) => (
          <li key={i} className={`chat-entry chat-entry--${entry.role}`}>
            <div className="chat-entry-content">
              <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: PopupMarkdownLink }}>
                {entry.content}
              </ReactMarkdown>
            </div>
            {!inThread && entry.id && entry.role !== "error" && (
              // P125 — a message the hub already has an id for can start a thread, or show the one it has.
              <div className="chat-entry-thread">
                {threads[entry.id] && threads[entry.id].replies > 0 ? (
                  <button type="button" className="link-button" onClick={() => onOpenThread(entry.id!)}>
                    {repliesLabel(threads[entry.id].replies)}
                  </button>
                ) : (
                  <button type="button" className="link-button" onClick={() => onOpenThread(entry.id!)}>
                    Responder em thread
                  </button>
                )}
              </div>
            )}
          </li>
        ))}
        {pending && live && <LiveBubble live={live} />}
        {pending && !live && <li className="chat-entry chat-entry--pending">…</li>}
      </ul>

      <form className="chat-composer" onSubmit={handleSubmit}>
        <input value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Mensagem" disabled={pending} />
        <button type="submit" disabled={pending || !draft.trim()}>
          Enviar
        </button>
      </form>
    </div>
  );
}
