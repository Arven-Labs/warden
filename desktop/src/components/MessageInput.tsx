import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { AttachIcon, CloseIcon, MicIcon, SendIcon, StopIcon } from "./Icons";
import { checkAttachmentLimits, isPdf } from "../lib/attachments";
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
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    textareaRef.current?.focus();
  }, [focusKey]);

  function submit() {
    const content = draft.trim();
    if ((content === "" && attachments.length === 0) || disabled) return;
    onSend(content, attachments);
    setDraft("");
    setAttachments([]);
  }

  async function handleAttach() {
    setAttachError(null);
    const selected = await open({
      multiple: true,
      filters: [{ name: "Images and PDFs", extensions: ["png", "jpg", "jpeg", "webp", "gif", "pdf"] }],
    });
    if (!selected) return;

    // What is already in the box counts toward the hub's limits; `taken` follows it through the loop, as the state lags.
    let taken = attachments;
    const paths = Array.isArray(selected) ? selected : [selected];
    for (const path of paths) {
      try {
        const attachment = await invoke<Attachment>("read_attachment", { path });
        const refused = checkAttachmentLimits(taken, attachment);
        if (refused) {
          setAttachError(refused);
          continue;
        }
        taken = [...taken, attachment];
        setAttachments(taken);
      } catch (err) {
        setAttachError(String(err));
      }
    }
  }

  function removeAttachment(index: number) {
    setAttachments((prev) => prev.filter((_, i) => i !== index));
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
    <div className="chat-input-dock">
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
                aria-label="Remove attachment"
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
        <button type="button" className="chat-attach-btn" aria-label="Attach image or PDF" onClick={handleAttach} disabled={disabled || isRecording}>
          <AttachIcon size={17} />
        </button>
        <button
          type="button"
          className={`chat-mic-btn${isRecording ? " chat-mic-btn--recording" : ""}`}
          aria-label={isRecording ? "Stop recording" : "Record voice message"}
          onClick={handleMicClick}
          disabled={disabled || isTranscribing}
        >
          <MicIcon size={17} />
        </button>
        <textarea
          ref={textareaRef}
          className="chat-input-textarea"
          aria-label="Message"
          placeholder={isTranscribing ? "Transcribing…" : "Message Warden…"}
          value={draft}
          onChange={(e) => setDraft(e.currentTarget.value)}
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
          <button type="button" className="chat-send-btn chat-stop-btn" aria-label="Stop" title="Stop: keeps what has been written" onClick={onStop}>
            <StopIcon size={15} />
          </button>
        ) : (
          <button
            type="submit"
            className="chat-send-btn"
            aria-label="Send message"
            disabled={disabled || (draft.trim() === "" && attachments.length === 0)}
          >
            <SendIcon size={17} />
          </button>
        )}
      </form>
      <p className="chat-input-hint">Warden can make mistakes. Check important info.</p>
    </div>
  );
}

export default MessageInput;
