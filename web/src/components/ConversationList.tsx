import { useState } from "react";
import { groupConversations } from "../hub/conversationGroups";
import type { ConversationSummary, ProjectDto } from "../hub/messages";
import { shortDate } from "../i18n/format";
import { t } from "../i18n";

interface Props {
  conversations: ConversationSummary[];
  /** The projects that exist (P103): the conversations started in one are listed under its name. */
  projects: ProjectDto[];
  activeId: string;
  /** Conversations with a turn waiting for its answer. */
  pendingIds: string[];
  error: string | null;
  /** The connection is down — browsing is fine, changing anything on the hub isn't. */
  disabled: boolean;
  onOpen: (id: string) => void;
  onNew: () => void;
  onRename: (id: string, title: string) => Promise<void>;
  onDelete: (id: string) => Promise<void>;
  /** Pins or archives a conversation (P157); a flag left out stays as it is. */
  onFlags: (id: string, flags: { pinned?: boolean; archived?: boolean }) => Promise<void>;
}

/** This device's conversations on the hub (P78): open, start, rename, delete, pin and archive (P157). A sidebar on wide
 * screens, a drawer on phones (`.chat-layout` in `App.css`). The pinned ones come first, the ones started in a project (P103) are
 * grouped under its name, the others by how recently they were used, and the archived ones are folded away at the end. */
export default function ConversationList({ conversations, projects, activeId, pendingIds, error, disabled, onOpen, onNew, onRename, onDelete, onFlags }: Props) {
  const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [archiveOpen, setArchiveOpen] = useState(false);

  async function setFlags(conversation: ConversationSummary, flags: { pinned?: boolean; archived?: boolean }) {
    setBusyId(conversation.id);
    await onFlags(conversation.id, flags);
    setBusyId(null);
  }

  async function saveRename() {
    if (!editing) return;
    const { id, title } = editing;
    const current = conversations.find((c) => c.id === id);
    if (!title.trim() || title.trim() === current?.title) {
      setEditing(null);
      return;
    }
    setBusyId(id);
    await onRename(id, title);
    setBusyId(null);
    setEditing(null);
  }

  async function remove(conversation: ConversationSummary) {
    if (!window.confirm(t("convs.confirmDelete", { title: conversation.title }))) return;
    setBusyId(conversation.id);
    await onDelete(conversation.id);
    setBusyId(null);
  }

  function renderItem(conversation: ConversationSummary) {
    const active = conversation.id === activeId;
    const answering = pendingIds.includes(conversation.id);
    // Also covers a conversation started here whose first turn hasn't reached the hub's disk yet.
    const locked = busyId === conversation.id || disabled || answering;
    if (editing?.id === conversation.id) {
      return (
        <li key={conversation.id} className="conversation conversation--editing">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void saveRename();
            }}
          >
            <input
              autoFocus
              value={editing.title}
              maxLength={120}
              aria-label={t("convs.newTitle")}
              disabled={busyId === conversation.id}
              onChange={(e) => setEditing({ id: conversation.id, title: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Escape") setEditing(null);
              }}
            />
            <div className="conversation-actions">
              <button type="submit" className="link-button" disabled={locked}>
                {t("convs.save")}
              </button>
              <button type="button" className="link-button" onClick={() => setEditing(null)}>
                {t("convs.cancel")}
              </button>
            </div>
          </form>
        </li>
      );
    }
    return (
      <li key={conversation.id} className={active ? "conversation conversation--active" : "conversation"}>
        <button type="button" className="conversation-open" onClick={() => onOpen(conversation.id)} aria-current={active ? "true" : undefined}>
          <span className="conversation-title">{conversation.title || t("convs.untitled")}</span>
          <span className="conversation-meta">{answering ? t("convs.answering") : shortDate(conversation.updatedAt)}</span>
        </button>
        <div className="conversation-actions">
          {!conversation.archived && (
            <button type="button" className="link-button" disabled={locked} onClick={() => void setFlags(conversation, { pinned: !conversation.pinned })}>
              {conversation.pinned ? t("convs.unpin") : t("convs.pin")}
            </button>
          )}
          <button type="button" className="link-button" disabled={locked} onClick={() => void setFlags(conversation, { archived: !conversation.archived })}>
            {conversation.archived ? t("convs.unarchive") : t("convs.archive")}
          </button>
          <button type="button" className="link-button" disabled={locked} onClick={() => setEditing({ id: conversation.id, title: conversation.title })}>
            {t("convs.rename")}
          </button>
          <button type="button" className="link-button skills-danger" disabled={locked} onClick={() => void remove(conversation)}>
            {t("convs.delete")}
          </button>
        </div>
      </li>
    );
  }

  // Pinned ones are listed first and archived ones apart, wherever their project is; the rest stay in their project's group or, loose,
  // in blocks by date.
  const { pinned, archived } = groupConversations(conversations, Date.now());
  const rest = conversations.filter((c) => !c.pinned && !c.archived);
  // A conversation whose project no longer exists (removed since) is listed with the others, as an ordinary one.
  const projectIds = new Set(projects.map((p) => p.id));
  const loose = rest.filter((c) => !c.projectId || !projectIds.has(c.projectId));
  const { buckets } = groupConversations(loose, Date.now());
  const groups = projects
    .map((project) => ({ project, items: rest.filter((c) => c.projectId === project.id) }))
    .filter((group) => group.items.length > 0);

  return (
    <aside className="conversations" aria-label={t("convs.sectionLabel")}>
      <button type="button" className="primary-button conversations-new" onClick={onNew}>
        {t("convs.new")}
      </button>
      {error && <p className="error-banner">{error}</p>}
      {conversations.length === 0 ? (
        <p className="conversations-empty">{t("convs.none")}</p>
      ) : (
        <>
          {pinned.length > 0 && (
            <section className="conversation-group" aria-label={t("convs.pinned")}>
              <h3 className="conversation-group-title">{t("convs.pinnedTitle")}</h3>
              <ul className="conversations-list">{pinned.map(renderItem)}</ul>
            </section>
          )}
          {groups.map(({ project, items }) => (
            <section className="conversation-group" key={project.id} aria-label={t("convs.projectLabel", { name: project.name })}>
              <h3 className="conversation-group-title" title={project.description || project.name}>
                {project.name}
              </h3>
              <ul className="conversations-list">{items.map(renderItem)}</ul>
            </section>
          ))}
          {buckets.map(({ label, items }) => (
            <section className="conversation-group" key={label} aria-label={label}>
              <h3 className="conversation-group-title">{label}</h3>
              <ul className="conversations-list">{items.map(renderItem)}</ul>
            </section>
          ))}
          {archived.length > 0 && (
            <section className="conversation-group" aria-label={t("convs.archived")}>
              <h3 className="conversation-group-title">
                <button type="button" className="conversation-archive-toggle" aria-expanded={archiveOpen} onClick={() => setArchiveOpen((open) => !open)}>
                  {archiveOpen ? "▾" : "▸"} {t("convs.archivedToggle", { count: archived.length })}
                </button>
              </h3>
              {archiveOpen && <ul className="conversations-list">{archived.map(renderItem)}</ul>}
            </section>
          )}
        </>
      )}
    </aside>
  );
}
