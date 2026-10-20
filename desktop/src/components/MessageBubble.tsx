import { useRef, useState } from "react";
import type { AnchorHTMLAttributes } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import type { Attachment, ChatMessage } from "../types";
import { LogoMark, SpeakerIcon, StopIcon } from "./Icons";
import { isPdf } from "../lib/attachments";
import { stripMarkdown } from "../lib/stripMarkdown";
import { repliesLabel } from "../lib/threads";
import CopyButton from "./CopyButton";
import Markdown from "./Markdown";

interface MessageBubbleProps {
  message: ChatMessage;
  /** The thread of this message on a hub (P125): `replies` is how many messages the person sent in it, or null when there is none yet. */
  thread?: { replies: number | null; onOpen: () => void };
  /** Regenerate the answer (P157): only given to the last answer, when the conversation can be redone. */
  onRegenerate?: () => void;
  /** Edit this message of the person's and send it again (P157): `after` is how many messages follow it, which the edit drops. */
  edit?: { after: number; onSend: (content: string) => void };
}

/** "Reply in thread", or the chip with the replies a thread already has. */
function ThreadButton({ thread }: { thread: { replies: number | null; onOpen: () => void } }) {
  return (
    <button type="button" className="message-thread-btn" onClick={thread.onOpen}>
      {thread.replies === null ? t("bubble.replyInThread") : repliesLabel(thread.replies)}
    </button>
  );
}

type SpeechState = "idle" | "loading" | "playing";

/** Per-message play button for TTS (P28 part 3) — synthesizes on click via the same Whisper API
 * key as voice input (`synthesize_speech` IPC), plays it, and toggles back to idle on a second
 * click (always restarts from the top next time, not a pause/resume). */
function SpeakButton({ text }: { text: string }) {
  const [state, setState] = useState<SpeechState>("idle");
  const [error, setError] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  async function handleClick() {
    if (state === "playing") {
      audioRef.current?.pause();
      if (audioRef.current) audioRef.current.currentTime = 0;
      setState("idle");
      return;
    }

    setError(null);
    setState("loading");
    try {
      const audio = await invoke<Attachment>("synthesize_speech", { text: stripMarkdown(text) });
      const element = new Audio(`data:${audio.mimeType};base64,${audio.data}`);
      element.onended = () => setState("idle");
      audioRef.current = element;
      await element.play();
      setState("playing");
    } catch (err) {
      setError(String(err));
      setState("idle");
    }
  }

  return (
    <>
      <button
        type="button"
        className={`message-speak-btn${state === "playing" ? " message-speak-btn--playing" : ""}`}
        aria-label={state === "playing" ? t("bubble.stopSpeaking") : t("bubble.speak")}
        onClick={handleClick}
        disabled={state === "loading"}
      >
        {state === "playing" ? <StopIcon size={14} /> : <SpeakerIcon size={14} />}
      </button>
      {error && <p className="chat-attach-error" role="alert">{error}</p>}
    </>
  );
}

/** Renders one `Attachment` as the right native media element for its `mimeType` — an `<img>`
 * for images (the only kind before P64 frente 2, user-attached only), and native browser
 * controls (no custom player needed, unlike `SpeakButton`) for audio/video that an MCP tool
 * produced during a turn. */
function AttachmentPreview({ attachment }: { attachment: Attachment }) {
  const src = `data:${attachment.mimeType};base64,${attachment.data}`;
  if (attachment.mimeType.startsWith("audio/")) {
    return <audio controls src={src} />;
  }
  if (attachment.mimeType.startsWith("video/")) {
    return <video controls src={src} />;
  }
  // A PDF is a file to open, not a picture (P157).
  if (isPdf(attachment)) {
    return (
      <a className="message-attachment-pdf" href={src} download="attachment.pdf">
        📄 PDF
      </a>
    );
  }
  return <img src={src} alt="" />;
}

/** One "Open" button for a file `generate_document`/oversized MCP media (P64) wrote to disk this
 * turn — hands the path to the Rust `open_generated_file` command, which opens it with the OS
 * default app (and refuses anything outside the trusted generated-files directory). Same
 * inline-error-without-blocking-dialog pattern as `SpeakButton` above. */
function GeneratedFileButton({ path }: { path: string }) {
  const [error, setError] = useState<string | null>(null);
  const name = path.split(/[/\\]/).pop() || path;

  async function handleClick() {
    setError(null);
    try {
      await invoke("open_generated_file", { path });
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <>
      <button type="button" className="message-file-btn" onClick={handleClick}>
        {t("bubble.openFile", { name })}
      </button>
      {error && <p className="chat-attach-error" role="alert">{error}</p>}
    </>
  );
}

// Links must open in the user's default browser, not navigate the app's own webview away.
// Exported for reuse by `VaultView` (P52 part 2), which renders markdown outside chat bubbles.
export function MarkdownLink(props: AnchorHTMLAttributes<HTMLAnchorElement>) {
  const { href, children, ...rest } = props;
  return (
    <a
      {...rest}
      href={href}
      onClick={(event) => {
        event.preventDefault();
        if (href) void openUrl(href);
      }}
    >
      {children}
    </a>
  );
}

/** The message of the person's turned into a box to change it (P157). Send asks first when the edit also drops what followed. */
function EditBox({ message, after, onSend, onCancel }: { message: ChatMessage; after: number; onSend: (content: string) => void; onCancel: () => void }) {
  const [text, setText] = useState(message.content);
  const hasMedia = (message.attachments?.length ?? 0) > 0;

  function send() {
    const content = text.trim();
    if (content === "" && !hasMedia) return;
    if (after > 0 && !window.confirm(after === 1 ? t("bubble.confirmEditOne") : t("bubble.confirmEditMany", { count: after }))) return;
    onSend(content);
  }

  return (
    <div className="message-edit">
      <textarea
        className="message-edit-input"
        value={text}
        autoFocus
        aria-label={t("bubble.editLabel")}
        rows={Math.min(10, Math.max(2, text.split("\n").length))}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) send();
        }}
      />
      <div className="message-edit-actions">
        <button type="button" className="message-edit-btn" onClick={onCancel}>
          {t("bubble.cancel")}
        </button>
        <button type="button" className="message-edit-btn message-edit-btn--send" onClick={send}>
          {t("bubble.send")}
        </button>
      </div>
    </div>
  );
}

function MessageBubble({ message, thread, onRegenerate, edit }: MessageBubbleProps) {
  const [editing, setEditing] = useState(false);
  if (message.role === "assistant") {
    return (
      <div className="message-row message-row--assistant">
        <div className="message-avatar">
          <LogoMark size={18} />
        </div>
        <div className="message-assistant-body">
          {message.fallbacks?.map((f, index) => (
            <p key={index} className="message-bubble-fallback">
              {t("bubble.answeredBy", { to: f.to, from: f.from, reason: f.reason })}
            </p>
          ))}
          {message.attachments && message.attachments.length > 0 && (
            <div className="message-bubble-attachments">
              {message.attachments.map((attachment, index) => (
                <AttachmentPreview key={index} attachment={attachment} />
              ))}
            </div>
          )}
          <div className="message-bubble-content">
            <Markdown link={MarkdownLink}>{message.content}</Markdown>
          </div>
          {message.generatedFiles && message.generatedFiles.length > 0 && (
            <div className="message-bubble-files">
              {message.generatedFiles.map((path, index) => (
                <GeneratedFileButton key={index} path={path} />
              ))}
            </div>
          )}
          <div className="message-bubble-footer">
            <SpeakButton text={message.content} />
            <CopyButton text={message.content} className="message-copy-btn" />
            {onRegenerate && (
              <button type="button" className="message-redo-btn" onClick={onRegenerate}>
                {t("bubble.regenerate")}
              </button>
            )}
            {message.usage && <span className="message-bubble-usage">{t("bubble.tokens", { count: message.usage.totalTokens })}</span>}
            {thread && <ThreadButton thread={thread} />}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="message-row message-row--user">
      <div className="message-bubble message-bubble--user">
        {message.attachments && message.attachments.length > 0 && (
          <div className="message-bubble-attachments">
            {message.attachments.map((attachment, index) => (
              <AttachmentPreview key={index} attachment={attachment} />
            ))}
          </div>
        )}
        {editing && edit ? (
          <EditBox
            message={message}
            after={edit.after}
            onSend={(content) => {
              setEditing(false);
              edit.onSend(content);
            }}
            onCancel={() => setEditing(false)}
          />
        ) : (
          message.content && (
            <div className="message-bubble-content">
              <Markdown link={MarkdownLink}>{message.content}</Markdown>
            </div>
          )
        )}
        {!editing && (thread || edit) && (
          <div className="message-bubble-footer">
            {edit && (
              <button type="button" className="message-redo-btn" onClick={() => setEditing(true)}>
                {t("bubble.edit")}
              </button>
            )}
            {thread && <ThreadButton thread={thread} />}
          </div>
        )}
      </div>
    </div>
  );
}

export default MessageBubble;
