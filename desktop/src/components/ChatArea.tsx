import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { t } from "../i18n";
import { nextCodeMode } from "../types";
import type { AgentEntry, Attachment, CodeMode, Combo, Conversation, ProjectEntry, ProviderEntry } from "../types";
import type { LiveTurn } from "../lib/liveTurn";
import { canRedo, lastUserMessage, messagesAfter } from "../lib/regenerate";
import { threadAnchor, type ThreadInfo } from "../lib/threads";
import { folderLabel, folderPlace, nodesWithFolders, type DirListing, type NodeInfo } from "../lib/workdir";
import FolderPicker from "./FolderPicker";
import { LogoMark } from "./Icons";
import Markdown from "./Markdown";
import MessageBubble, { MarkdownLink } from "./MessageBubble";
import MessageInput from "./MessageInput";

interface ChatAreaProps {
  activeConversation: Conversation | undefined;
  onSendMessage: (content: string, attachments: Attachment[]) => void;
  /** Sends a turn that takes the place of a message of the conversation and of all that followed (P157): "regenerate" and "edit".
   * Absent where a conversation can't be redone. */
  onRedo?: (content: string, attachments: Attachment[], replaceFrom: string) => void;
  isSending: boolean;
  /** What the code engine is doing in the turn being sent (P103 b), or the tools of the tasks an agent started (P124); none until it says something. */
  live?: LiveTurn;
  /** The Stop button (P157): ends the turn being answered where it is; what was written is kept as the answer. Shown while sending. */
  onCancel?: () => void;
  /** How much a code conversation asks before the engine acts (P103 b); the picker shows only in a code project. */
  codeMode: CodeMode;
  onCodeMode: (mode: CodeMode) => void;
  sendError: string | null;
  agents: AgentEntry[];
  providers: ProviderEntry[];
  /** Combos (P90) are picked here like providers. */
  combos: Combo[];
  /** Projects (P103) a new conversation can start in. */
  projects: ProjectEntry[];
  /** The conversation's project, or the one a new conversation will start in; "" = none. */
  selectedProjectId: string;
  /** Chooses where a conversation that hasn't started will begin. */
  onSelectProject: (projectId: string) => void;
  /** Moves a conversation that has started into a project ("" = out of any). */
  onMoveProject: (projectId: string) => void;
  /** This computer's conversation in a code project is on the hub (P124): the phone and the web see it. */
  sharedWithHub?: boolean;
  /** Shares the conversation with the hub, or stops sharing it. Only given for a conversation of this computer. */
  onShare?: (shared: boolean) => void;
  /** The folder of this computer the conversation works in (P102), or the one a new conversation will start in; "" = none. */
  selectedWorkdir: string;
  /** Chooses the folder of a conversation that hasn't started ("" = none). After its first message it is fixed. */
  onSelectWorkdir: (folder: string) => void;
  selectedAgentId: string;
  selectedProviderId: string;
  onSelectAgent: (agentId: string) => void;
  onSelectProvider: (providerId: string) => void;
  /** The conversation runs on a hub (P102), not on this computer: the hub picks the model from the agent (a turn can't
   * name one), and its folders are browsed on the hub instead of with this computer's dialog. */
  remote?: boolean;
  /** What browsing a hub's folders needs (only with `remote`): the listing, the nodes known (to name them), and a call
   * made before the browser opens that reads the nodes (the owner's; a member has none to read). */
  hubFolders?: { listDirs: (path?: string) => Promise<DirListing>; nodes: NodeInfo[]; prepare: () => Promise<void> };
  /** Opens the thread of a message of this conversation (P125): on a hub the id is the one the hub gave the message, on this computer the
   * message's own. Absent where there are no threads (a code project). */
  onOpenThread?: (messageId: string) => void;
  /** The threads of this conversation by the id of the message they came from: the chip with the replies in place of the button. */
  threads?: Record<string, ThreadInfo>;
  /** The conversation is an agent's channel (P121): the agent is the channel's, there is no pick screen, project or folder; only the chat. */
  channel?: boolean;
  /** With `channel`: what the header says instead of the agent's name (a conversation of the agent outside its channel). */
  channelTitle?: string;
  /** With `channel`: a button beside the header's label (to the agent's notes and runs, or back to its channel). */
  headerAction?: { label: string; onClick: () => void };
}

function ThinkingIndicator() {
  return (
    <div className="message-row message-row--assistant">
      <div className="message-avatar">
        <LogoMark size={18} />
      </div>
      <div className="thinking-indicator" aria-label={t("chat.thinking")}>
        <span />
        <span />
        <span />
      </div>
    </div>
  );
}

const TOOL_MARK = { running: "…", completed: "✓", failed: "✗" } as const;

/** The task a code engine is running: its words and its tools in the order they happened, as they happen. */
function LiveBubble({ live }: { live: LiveTurn }) {
  return (
    <div className="message-row message-row--assistant">
      <div className="message-avatar">
        <LogoMark size={18} />
      </div>
      <div className="message-assistant-body bubble--live" aria-label={t("chat.inProgress")} aria-live="polite">
        {live.items.map((item, i) =>
          item.kind === "text" ? (
            <div key={i} className="message-bubble-content">
              {/* Words of a manager's task say which one, since several can be writing at once. */}
              {item.source && <p className="live-source">{item.source}</p>}
              <Markdown link={MarkdownLink}>{item.text}</Markdown>
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
      </div>
    </div>
  );
}

function ChatArea({
  activeConversation,
  onSendMessage,
  onRedo,
  isSending,
  live,
  onCancel,
  codeMode,
  onCodeMode,
  sendError,
  agents,
  providers,
  combos,
  projects,
  selectedProjectId,
  onSelectProject,
  onMoveProject,
  sharedWithHub = false,
  onShare,
  selectedWorkdir,
  onSelectWorkdir,
  selectedAgentId,
  selectedProviderId,
  onSelectAgent,
  onSelectProvider,
  remote = false,
  hubFolders,
  onOpenThread,
  threads,
  channel = false,
  channelTitle,
  headerAction,
}: ChatAreaProps) {
  // The hub's folder browser (P102), open while the person chooses.
  const [browsingHub, setBrowsingHub] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);
  // A project picked for a conversation that has started waits for a yes: moving it changes what the AI can reach.
  const [pendingMove, setPendingMove] = useState<string | null>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth", block: "end" });
  }, [activeConversation?.messages.length, isSending, live]);

  // Another conversation, another question.
  useEffect(() => setPendingMove(null), [activeConversation?.id]);

  const hasMessages = !!activeConversation && activeConversation.messages.length > 0;
  // A conversation starts with no agent, ready to type in (P157), and the agent can be changed from the header at any time (P124).
  // A conversation's project, unlike the agent, is chosen before its first message and then fixed (P103). One whose
  // project was removed since has nothing to show.
  const knownProject = projects.some((p) => p.id === selectedProjectId);
  // A conversation that works in a folder can't be moved into a project (P102), so it has no project picker.
  const showProjectPicker = !channel && (projects.length > 0 || knownProject) && !(hasMessages && selectedWorkdir);
  // The folder (P102): picked before the first message — with the system's own dialog on this computer, in the hub's
  // folder browser on a hub — then only shown. Not inside a project, which has its own.
  const hubNodes = hubFolders?.nodes ?? [];
  const folderName = remote ? folderLabel(selectedWorkdir, hubNodes) : (selectedWorkdir.split("/").filter(Boolean).pop() ?? selectedWorkdir);
  const folderTitle = remote ? folderPlace(selectedWorkdir, hubNodes) : selectedWorkdir;
  const showFolder = !channel && !knownProject && (hasMessages ? selectedWorkdir !== "" : true) && (!remote || hubFolders !== undefined);

  async function chooseFolder() {
    if (remote && hubFolders) {
      // The nodes are read first so the browser can offer them (the owner's; a member's call is a no-op).
      await hubFolders.prepare();
      setBrowsingHub(true);
      return;
    }
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: selectedWorkdir || undefined });
      if (typeof picked === "string") onSelectWorkdir(picked);
    } catch (err) {
      console.error("failed to pick a folder:", err);
    }
  }

  return (
    <div className="chat-area">
      {browsingHub && hubFolders && (
        <FolderPicker
          listDirs={hubFolders.listDirs}
          nodes={nodesWithFolders(hubNodes)}
          initialPath={selectedWorkdir || undefined}
          onPick={(path) => {
            onSelectWorkdir(path);
            setBrowsingHub(false);
          }}
          onCancel={() => setBrowsingHub(false)}
        />
      )}
      <div className="chat-header">
        {channel ? (
          // An agent's channel is that agent's for good: no switching it here.
          <>
            {headerAction && channelTitle !== undefined && (
              <button type="button" className="chat-header-action" onClick={headerAction.onClick}>
                {headerAction.label}
              </button>
            )}
            <span className="chat-header-label" title={t("chat.channelOf")}>
              {channelTitle ?? selectedAgentId}
            </span>
            {headerAction && channelTitle === undefined && (
              <button type="button" className="chat-header-action" onClick={headerAction.onClick}>
                {headerAction.label}
              </button>
            )}
          </>
        ) : agents.length > 0 || selectedAgentId ? (
          // The agent can be changed at any time (P124): the next messages speak as the new one. Locked while an answer is on the way.
          <select
            className="chat-header-select"
            aria-label={t("chat.agent")}
            title={t("chat.agentTitle")}
            value={selectedAgentId}
            disabled={isSending}
            onChange={(e) => {
              onSelectAgent(e.currentTarget.value);
            }}
          >
            <option value="">{t("chat.noAgent")}</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id}>
                {a.id}
              </option>
            ))}
            {selectedAgentId && !agents.some((a) => a.id === selectedAgentId) && <option value={selectedAgentId}>{t("chat.agentRemoved", { id: selectedAgentId })}</option>}
          </select>
        ) : (
          <span className="chat-header-label">{t("chat.noAgent")}</span>
        )}
        {showProjectPicker && (
          <select
            className="chat-header-select"
            aria-label={t("chat.project")}
            title={t("chat.projectTitle")}
            value={knownProject ? selectedProjectId : ""}
            disabled={isSending}
            onChange={(e) => {
              const next = e.currentTarget.value;
              if (!hasMessages) onSelectProject(next);
              else if (next !== (knownProject ? selectedProjectId : "")) setPendingMove(next);
            }}
          >
            <option value="">{t("chat.noProject")}</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        )}
        {showFolder &&
          (hasMessages ? (
            <span className="chat-header-label" title={folderTitle}>
              {t("chat.folder", { name: folderName })}
            </span>
          ) : (
            <span className="chat-header-folder">
              <button
                type="button"
                className="chat-header-select"
                title={
                  folderTitle ||
                  (remote ? t("chat.folderHintHub") : t("chat.folderHintLocal"))
                }
                disabled={isSending}
                onClick={() => void chooseFolder()}
              >
                {selectedWorkdir ? t("chat.folder", { name: folderName }) : t("chat.noFolder")}
              </button>
              {selectedWorkdir && (
                <button type="button" className="chat-header-select" aria-label={t("chat.clearFolder")} onClick={() => onSelectWorkdir("")}>
                  ×
                </button>
              )}
            </span>
          ))}
        {projects.some((p) => p.id === selectedProjectId && p.code) && (
          // Changeable at any moment, a task that is running included (P103 b).
          <select
            className={`chat-header-select${codeMode === "acceptAll" ? " chat-header-select--warn" : ""}`}
            aria-label={t("chat.mode")}
            title={t("chat.modeTitle")}
            value={codeMode}
            onChange={(e) => onCodeMode(e.currentTarget.value as CodeMode)}
          >
            <option value="manual">{t("chat.modeManual")}</option>
            <option value="acceptEdits">{t("chat.modeAcceptEdits")}</option>
            <option value="acceptAll">{t("chat.modeAcceptAll")}</option>
            <option value="plan">{t("chat.modePlan")}</option>
          </select>
        )}
        {onShare && !remote && hasMessages && projects.some((p) => p.id === selectedProjectId && p.code) && (
          <button
            type="button"
            className="chat-header-select"
            title={sharedWithHub ? t("chat.sharedTitle") : t("chat.shareTitle")}
            disabled={isSending}
            onClick={() => onShare(!sharedWithHub)}
          >
            {sharedWithHub ? t("chat.shared") : t("chat.share")}
          </button>
        )}
        {/* With no provider and no combo there is nothing to pick: the box would be an empty pill. */}
        {!remote && providers.length + combos.length > 0 && (
          <select
            className="chat-header-select"
            aria-label={t("chat.model")}
            value={selectedProviderId}
            onChange={(e) => onSelectProvider(e.currentTarget.value)}
          >
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.id}
              </option>
            ))}
            {combos.map((c) => (
              <option key={c.id} value={c.id} title={c.providers.join(" → ")}>
                {t("chat.combo", { id: c.id })}
              </option>
            ))}
          </select>
        )}
      </div>
      {pendingMove !== null && (
        <div className="chat-move-banner" role="alert">
          <span>
            {pendingMove === ""
              ? t("chat.moveOut")
              : t("chat.moveTo", { project: projects.find((p) => p.id === pendingMove)?.name ?? pendingMove })}
          </span>
          <button
            type="button"
            className="settings-save-btn"
            onClick={() => {
              onMoveProject(pendingMove);
              setPendingMove(null);
            }}
          >
            {t("chat.move")}
          </button>
          <button type="button" className="settings-browse-btn" onClick={() => setPendingMove(null)}>
            {t("chat.cancel")}
          </button>
        </div>
      )}
      <div className="chat-messages" role="log" aria-live="polite" aria-label={t("chat.messages")}>
        {!hasMessages ? (
          <div className="chat-empty-state">
            <LogoMark size={40} />
            <h1>{t("chat.empty")}</h1>
          </div>
        ) : (
          <div className="chat-messages-column">
            {activeConversation!.messages.map((message, index, all) => {
              const anchor = onOpenThread ? threadAnchor(message, !remote) : null;
              // Regenerate and edit (P157): not in a channel, a thread or a code project, and not while a turn is answered.
              const redoable = onRedo !== undefined && !channel && canRedo(activeConversation, projects.some((p) => p.id === selectedProjectId && p.code), isSending);
              const lastUser = lastUserMessage(all);
              return (
                <MessageBubble
                  key={message.id}
                  message={message}
                  thread={anchor ? { replies: threads?.[anchor]?.replies ?? null, onOpen: () => onOpenThread!(anchor) } : undefined}
                  onRegenerate={
                    redoable && message.role === "assistant" && index === all.length - 1 && lastUser
                      ? () => onRedo(lastUser.content, lastUser.attachments ?? [], lastUser.id)
                      : undefined
                  }
                  edit={
                    redoable && message.role === "user"
                      ? { after: messagesAfter(all, message.id), onSend: (content) => onRedo(content, message.attachments ?? [], message.id) }
                      : undefined
                  }
                />
              );
            })}
            {isSending && live && <LiveBubble live={live} />}
            {isSending && !live && <ThinkingIndicator />}
            <div ref={bottomRef} />
          </div>
        )}
      </div>
      {sendError && (
        <div className="chat-error-banner" role="alert">
          {sendError}
        </div>
      )}
      <MessageInput
        onSend={onSendMessage}
        focusKey={activeConversation?.id ?? null}
        disabled={isSending}
        onStop={isSending ? onCancel : undefined}
        onCycleMode={projects.some((p) => p.id === selectedProjectId && p.code) ? () => onCodeMode(nextCodeMode(codeMode)) : undefined}
      />
    </div>
  );
}

export default ChatArea;
