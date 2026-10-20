import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { AttachIcon, CloseIcon, MicIcon, SendIcon, StopIcon } from "./Icons";
import { t } from "../i18n";
import { checkAttachmentLimits, fileToAttachment, isAttachmentFile, isPdf } from "../lib/attachments";
import type { Attachment } from "../types";

interface MessageInputProps {
  onSend: (content: string, attachments: Attachment[]) => void;
  /** Bumped by the parent whenever the active conversation changes, so the composer refocuses. */
  focusKey?: string | null;
  disabled?: boolean;
  /** Given while a turn is being answered: the send button becomes a Stop button that ends it where it is (P157). */
  onStop?: () => void;
  /** Shift+Tab in the box, as in Claude Code: the next mode of a code conversation. Absent when there is no mode to change. */
  onCycleMode?: () => void;
}

function MessageInput({ onSend, focusKey, disabled, onStop, onCycleMode }: MessageInputProps) {
  const [draft, setDraft] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [attachError, setAttachError] = useState<string | null>(null);
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [recordError, setRecordError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  // The attachments and `disabled` are also read from the window's drop handler, which is set up once: a ref is what it sees.
  const attachmentsRef = useRef<Attachment[]>([]);
  const disabledRef = useRef(disabled);
  disabledRef.current = disabled;

  useEffect(() => {
    textareaRef.current?.focus();
  }, [focusKey]);

  // The box grows with what is typed, up to the CSS `max-height`, and shrinks back when it is emptied (a sent message).
  useEffect(() => {
    const box = textareaRef.current;
    if (!box) return;
    box.style.height = "auto";
    box.style.height = `${box.scrollHeight}px`;
  }, [draft]);

  // A file dragged in from the desktop reaches the window as paths, not as DOM files (Tauri takes the drop itself), so it goes
  // through `read_attachment` like one picked with the button.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let gone = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const { type } = event.payload;
        if (type === "enter" || type === "over") setDragging(!disabledRef.current);
        else if (type === "leave") setDragging(false);
        else if (type === "drop") {
          setDragging(false);
          if (!disabledRef.current) void addAttachments(event.payload.paths.map((path) => () => invoke<Attachment>("read_attachment", { path })));
        }
      })
      .then((stop) => (gone ? stop() : (unlisten = stop)))
      .catch(() => undefined);
    return () => {
      gone = true;
      unlisten?.();
    };
  }, []);

  function setAllAttachments(next: Attachment[]) {
    attachmentsRef.current = next;
    setAttachments(next);
  }

  function submit() {
    const content = draft.trim();
    if ((content === "" && attachments.length === 0) || disabled) return;
    onSend(content, attachments);
    setDraft("");
    setAllAttachments([]);
  }

  /** Adds what each reader produces, one at a time. What is already in the box counts toward the hub's limits, so every file is
   * checked against the box as it is when its turn comes. A file that can't be read or doesn't fit says why and the rest go on. */
  async function addAttachments(readers: Array<() => Promise<Attachment>>) {
    setAttachError(null);
    for (const read of readers) {
      try {
        const attachment = await read();
        const refused = checkAttachmentLimits(attachmentsRef.current, attachment);
        if (refused) {
          setAttachError(refused);
          continue;
        }
        setAllAttachments([...attachmentsRef.current, attachment]);
      } catch (err) {
        setAttachError(String(err instanceof Error ? err.message : err));
      }
    }
  }

  async function handleAttach() {
    setAttachError(null);
    const selected = await open({
      multiple: true,
      filters: [{ name: t("input.filterName"), extensions: ["png", "jpg", "jpeg", "webp", "gif", "pdf"] }],
    });
    if (!selected) return;

    const paths = Array.isArray(selected) ? selected : [selected];
    await addAttachments(paths.map((path) => () => invoke<Attachment>("read_attachment", { path })));
  }

  /** A picture or a PDF on the clipboard (a screenshot, a copied image) goes in as an attachment; text pastes as it always did. */
  function handlePaste(event: React.ClipboardEvent<HTMLTextAreaElement>) {
    const files = [...event.clipboardData.files].filter(isAttachmentFile);
    if (files.length === 0) return;
    event.preventDefault();
    void addAttachments(files.map((file) => () => fileToAttachment(file)));
  }

  function removeAttachment(index: number) {
    setAllAttachments(attachmentsRef.current.filter((_, i) => i !== index));
  }

  async function handleMicClick() {
    setRecordError(null);
    if (!isRecording) {
      try {
        await invoke("start_recording");
        setIsRecording(true);
      } catch (err) {
        setRecordError(String(err));
      }
      return;
    }

    setIsRecording(false);
    setIsTranscribing(true);
    try {
      const audio = await invoke<Attachment>("stop_recording");
      const text = await invoke<string>("transcribe_audio", { audio });
      const trimmed = text.trim();
      if (trimmed !== "") {
        setDraft((prev) => (prev.trim() === "" ? trimmed : `${prev} ${trimmed}`));
        textareaRef.current?.focus();
      }
    } catch (err) {
      setRecordError(String(err));
    } finally {
      setIsTranscribing(false);
    }
  }

  return (
    <div className={`chat-input-dock${dragging ? " chat-input-dock--dragging" : ""}`}>
      {attachError && <p className="chat-attach-error" role="alert">{attachError}</p>}
      {recordError && <p className="chat-attach-error" role="alert">{recordError}</p>}
      {attachments.length > 0 && (
        <div className="chat-attachment-preview">
          {attachments.map((attachment, index) => (
            <div className="chat-attachment-thumb" key={index}>
              {isPdf(attachment) ? (
                <span className="chat-attachment-file">PDF</span>
              ) : (
                <img src={`data:${attachment.mimeType};base64,${attachment.data}`} alt="" />
              )}
              <button
                type="button"
                className="chat-attachment-remove"
                aria-label={t("input.removeAttachment")}
                onClick={() => removeAttachment(index)}
              >
                <CloseIcon size={11} />
              </button>
            </div>
          ))}
        </div>
      )}
      <form
        className="chat-input-form"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <button type="button" className="chat-attach-btn" aria-label={t("input.attach")} onClick={handleAttach} disabled={disabled || isRecording}>
          <AttachIcon size={17} />
        </button>
        <button
          type="button"
          className={`chat-mic-btn${isRecording ? " chat-mic-btn--recording" : ""}`}
          aria-label={isRecording ? t("input.stopRecording") : t("input.record")}
          onClick={handleMicClick}
          disabled={disabled || isTranscribing}
        >
          <MicIcon size={17} />
        </button>
        <textarea
          ref={textareaRef}
          className="chat-input-textarea"
          aria-label={t("input.message")}
          placeholder={isTranscribing ? t("input.transcribing") : t("input.placeholder")}
          value={draft}
          onChange={(e) => setDraft(e.currentTarget.value)}
          onPaste={handlePaste}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            } else if (e.key === "Tab" && e.shiftKey && onCycleMode) {
              e.preventDefault();
              onCycleMode();
            }
          }}
          disabled={disabled || isTranscribing}
          rows={1}
        />
        {onStop ? (
          <button type="button" className="chat-send-btn chat-stop-btn" aria-label={t("input.stop")} title={t("input.stopTitle")} onClick={onStop}>
            <StopIcon size={15} />
          </button>
        ) : (
          <button
            type="submit"
            className="chat-send-btn"
            aria-label={t("input.send")}
            disabled={disabled || (draft.trim() === "" && attachments.length === 0)}
          >
            <SendIcon size={17} />
          </button>
        )}
      </form>
      <p className="chat-input-hint">{t("input.hint")}</p>
    </div>
  );
}

export default MessageInput;
