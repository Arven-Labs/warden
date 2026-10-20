import { useState } from "react";
import { t } from "../i18n";
import { groupConversations } from "../lib/conversationGroups";
import { searchConversations } from "../lib/conversationSearch";
import type { Conversation, ProjectEntry } from "../types";
import { ActivityIcon, AgentsIcon, ChartIcon, ChevronIcon, ClockIcon, DevicesIcon, LogoMark, OrgIcon, PlusIcon, ProjectsIcon, SettingsIcon, SkillsIcon, SyncIcon, VaultIcon, WebhookIcon } from "./Icons";

interface SidebarProps {
  conversations: Conversation[];
  /** The projects that exist (P103): the list groups the conversations that were started in one. */
  projects: ProjectEntry[];
  activeConversationId: string | null;
  onSelectConversation: (id: string) => void;
  /** Gives a conversation a new title; the window shows the reason when it can't. */
  onRenameConversation: (id: string, title: string) => Promise<void>;
  /** Deletes a conversation (and its threads); the window shows the reason when it can't. */
  onDeleteConversation: (id: string) => Promise<void>;
  /** Pins or archives a conversation (P157); a flag left out stays as it is. The window shows the reason when it can't. */
  onSetConversationFlags: (id: string, flags: { pinned?: boolean; archived?: boolean }) => Promise<void>;
  onNewConversation: () => void;
  onOpenSettings: () => void;
  onOpenUsage: () => void;
  onOpenSync: () => void;
  onOpenVault: () => void;
  onOpenSkills: () => void;
  onOpenProjects: () => void;
  onOpenTasks: () => void;
  onOpenWebhooks: () => void;
  onOpenWorkspace: () => void;
  /** Omitted for a member the owner gave no access to the organization (P120). */
  onOpenOrganization?: () => void;
  onOpenAgentWork: () => void;
  /** The feed of activity (P121): what happened among the agents. */
  onOpenActivity: () => void;
  /** The screen of the agents' channels (P121), only on a hub: omitted on this computer, where there is none. */
  onOpenAgents?: () => void;
  /** How many agent channels have something the person has not seen (P121): shown on the Agents button. */
  agentsUnread?: number;
  view: "chat" | "agents" | "settings" | "usage" | "sync" | "vault" | "skills" | "projects" | "tasks" | "webhooks" | "workspace" | "organization" | "agentWork" | "activity";
  collapsed: boolean;
  onToggleCollapsed: () => void;
  /** Which machine the screens use (P102): this computer or a hub. Shown under the brand, not in the collapsed bar. */
  machine?: React.ReactNode;
}

function Sidebar({
  conversations,
  projects,
  activeConversationId,
  onSelectConversation,
  onRenameConversation,
  onDeleteConversation,
  onSetConversationFlags,
  onNewConversation,
  onOpenSettings,
  onOpenUsage,
  onOpenSync,
  onOpenVault,
  onOpenSkills,
  onOpenProjects,
  onOpenTasks,
  onOpenWebhooks,
  onOpenWorkspace,
  onOpenOrganization,
  onOpenAgentWork,
  onOpenActivity,
  onOpenAgents,
  agentsUnread = 0,
  view,
  collapsed,
  onToggleCollapsed,
  machine,
}: SidebarProps) {
  const [query, setQuery] = useState("");
  // The conversation being renamed and what is typed so far; `busyId` is the one a rename or delete is working on.
  const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  // The archived ones are folded away; searching opens them, since a search is how they are found.
  const [archiveOpen, setArchiveOpen] = useState(false);

  const shown = searchConversations(conversations, query);
  // Pinned ones are listed first and archived ones apart, wherever their project is (P157); the rest stay in their project's group
  // or, loose, in blocks by how recently they were used.
  const { pinned, archived } = groupConversations(shown, Date.now());
  const rest = shown.filter((c) => !c.pinned && !c.archived);
  // A conversation whose project no longer exists (removed since) is listed with the others, as an ordinary one.
  const projectIds = new Set(projects.map((p) => p.id));
  const loose = rest.filter((c) => !c.projectId || !projectIds.has(c.projectId));
  const { buckets } = groupConversations(loose, Date.now());
  const groups = projects
    .map((project) => ({ project, items: rest.filter((c) => c.projectId === project.id) }))
    .filter((group) => group.items.length > 0);

  async function setFlags(conversation: Conversation, flags: { pinned?: boolean; archived?: boolean }) {
    setBusyId(conversation.id);
    try {
      await onSetConversationFlags(conversation.id, flags);
    } finally {
      setBusyId(null);
    }
  }

  async function saveRename(conversation: Conversation, title: string) {
    const trimmed = title.trim();
    setEditing(null);
    // An empty or unchanged title only closes the box.
    if (trimmed === "" || trimmed === conversation.title) return;
    setBusyId(conversation.id);
    try {
      await onRenameConversation(conversation.id, trimmed);
    } finally {
      setBusyId(null);
    }
  }

  async function remove(conversation: Conversation) {
    if (!window.confirm(t("sidebar.confirmDelete", { title: conversation.title }))) return;
    setBusyId(conversation.id);
    try {
      await onDeleteConversation(conversation.id);
    } finally {
      setBusyId(null);
    }
  }

  const renderItem = (conversation: Conversation) => {
    if (editing?.id === conversation.id) {
      return (
        <li key={conversation.id}>
          <input
            className="conversation-rename-input"
            value={editing.title}
            maxLength={120}
            autoFocus
            aria-label={t("sidebar.titleLabel")}
            onChange={(e) => setEditing({ id: conversation.id, title: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveRename(conversation, editing.title);
              if (e.key === "Escape") setEditing(null);
            }}
            onBlur={() => setEditing(null)}
          />
        </li>
      );
    }
    const busy = busyId === conversation.id;
    return (
      <li key={conversation.id} className="conversation-row">
        <button
          type="button"
          className={
            "conversation-list-item" +
            (view === "chat" && conversation.id === activeConversationId ? " conversation-list-item--active" : "")
          }
          onClick={() => onSelectConversation(conversation.id)}
        >
          {conversation.title}
        </button>
        <span className="conversation-row-actions">
          {!conversation.archived && (
            <button
              type="button"
              className="conversation-row-action"
              title={conversation.pinned ? t("sidebar.unpin") : t("sidebar.pin")}
              aria-label={conversation.pinned ? t("sidebar.unpinLabel", { title: conversation.title }) : t("sidebar.pinLabel", { title: conversation.title })}
              disabled={busy}
              onClick={() => void setFlags(conversation, { pinned: !conversation.pinned })}
            >
              {conversation.pinned ? "★" : "☆"}
            </button>
          )}
          <button
            type="button"
            className="conversation-row-action"
            title={conversation.archived ? t("sidebar.unarchive") : t("sidebar.archive")}
            aria-label={conversation.archived ? t("sidebar.unarchiveLabel", { title: conversation.title }) : t("sidebar.archiveLabel", { title: conversation.title })}
            disabled={busy}
            onClick={() => void setFlags(conversation, { archived: !conversation.archived })}
          >
            {conversation.archived ? "↩" : "▤"}
          </button>
          <button
            type="button"
            className="conversation-row-action"
            title={t("sidebar.rename")}
            aria-label={t("sidebar.renameLabel", { title: conversation.title })}
            disabled={busy}
            onClick={() => setEditing({ id: conversation.id, title: conversation.title })}
          >
            ✎
          </button>
          <button
            type="button"
            className="conversation-row-action"
            title={t("sidebar.delete")}
            aria-label={t("sidebar.deleteLabel", { title: conversation.title })}
            disabled={busy}
            onClick={() => void remove(conversation)}
          >
            ✕
          </button>
        </span>
      </li>
    );
  };

  return (
    <div className={`sidebar${collapsed ? " sidebar--collapsed" : ""}`}>
      <div className="sidebar-header">
        <div className="sidebar-topbar">
          <div className="sidebar-brand">
            <LogoMark size={24} />
            {!collapsed && <span className="sidebar-title">Warden</span>}
          </div>
          <button
            type="button"
            className="sidebar-collapse-btn"
            onClick={onToggleCollapsed}
            aria-label={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
            title={collapsed ? t("sidebar.expand") : t("sidebar.collapse")}
          >
            <ChevronIcon size={14} />
          </button>
        </div>
        {!collapsed && machine}
        <button type="button" className="new-conversation-btn" onClick={onNewConversation} title={t("sidebar.newChat")}>
          <PlusIcon size={16} />
          {!collapsed && t("sidebar.newChat")}
        </button>
      </div>

      {!collapsed && (
        <div className="conversation-list-scroll">
          {conversations.length > 0 && (
            <input
              type="search"
              className="conversation-search"
              placeholder={t("sidebar.search")}
              aria-label={t("sidebar.search")}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") setQuery("");
              }}
            />
          )}
          {conversations.length === 0 ? (
            <p className="conversation-list-empty">{t("sidebar.empty")}</p>
          ) : shown.length === 0 ? (
            <p className="conversation-list-empty">{t("sidebar.noMatch")}</p>
          ) : (
            <>
              {pinned.length > 0 && (
                <div className="conversation-group">
                  <div className="conversation-group-title">{t("sidebar.pinned")}</div>
                  <ul className="conversation-list">{pinned.map(renderItem)}</ul>
                </div>
              )}
              {groups.map(({ project, items }) => (
                <div className="conversation-group" key={project.id}>
                  <div className="conversation-group-title" title={project.description || project.name}>
                    <ProjectsIcon size={13} />
                    {project.name}
                  </div>
                  <ul className="conversation-list">{items.map(renderItem)}</ul>
                </div>
              ))}
              {buckets.map(({ label, items }) => (
                <div className="conversation-group" key={label}>
                  <div className="conversation-group-title">{label}</div>
                  <ul className="conversation-list">{items.map(renderItem)}</ul>
                </div>
              ))}
              {archived.length > 0 && (
                <div className="conversation-group">
                  <button type="button" className="conversation-archive-toggle" aria-expanded={archiveOpen || query !== ""} onClick={() => setArchiveOpen((open) => !open)}>
                    {archiveOpen || query !== "" ? "▾" : "▸"} {t("sidebar.archived", { count: archived.length })}
                  </button>
                  {(archiveOpen || query !== "") && <ul className="conversation-list">{archived.map(renderItem)}</ul>}
                </div>
              )}
            </>
          )}
        </div>
      )}

      <div className="sidebar-footer">
        {onOpenAgents && (
          <button
            type="button"
            className={`sidebar-footer-btn${view === "agents" ? " sidebar-footer-btn--active" : ""}`}
            onClick={onOpenAgents}
            title={t("nav.agents")}
          >
            <AgentsIcon size={17} />
            {!collapsed && t("nav.agents")}
            {agentsUnread > 0 && (
              <span className="sidebar-badge" aria-label={t("nav.unread", { count: agentsUnread })}>
                {agentsUnread}
              </span>
            )}
          </button>
        )}
        <button
          type="button"
          className={`sidebar-footer-btn${view === "usage" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenUsage}
          title={t("nav.usage")}
        >
          <ChartIcon size={17} />
          {!collapsed && t("nav.usage")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "sync" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenSync}
          title={t("nav.sync")}
        >
          <SyncIcon size={17} />
          {!collapsed && t("nav.sync")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "vault" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenVault}
          title={t("nav.vault")}
        >
          <VaultIcon size={17} />
          {!collapsed && t("nav.vault")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "skills" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenSkills}
          title={t("nav.skills")}
        >
          <SkillsIcon size={17} />
          {!collapsed && t("nav.skills")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "projects" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenProjects}
          title={t("nav.projects")}
        >
          <ProjectsIcon size={17} />
          {!collapsed && t("nav.projects")}
        </button>
        {onOpenOrganization && (
          <button
            type="button"
            className={`sidebar-footer-btn${view === "organization" ? " sidebar-footer-btn--active" : ""}`}
            onClick={onOpenOrganization}
            title={t("nav.organization")}
          >
            <OrgIcon size={17} />
            {!collapsed && t("nav.organization")}
          </button>
        )}
        <button
          type="button"
          className={`sidebar-footer-btn${view === "agentWork" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenAgentWork}
          title={t("nav.agentWork")}
        >
          <ChartIcon size={17} />
          {!collapsed && t("nav.agentWork")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "activity" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenActivity}
          title={t("nav.activity")}
        >
          <ActivityIcon size={17} />
          {!collapsed && t("nav.activity")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "tasks" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenTasks}
          title={t("nav.tasks")}
        >
          <ClockIcon size={17} />
          {!collapsed && t("nav.tasks")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "webhooks" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenWebhooks}
          title={t("nav.webhooks")}
        >
          <WebhookIcon size={17} />
          {!collapsed && t("nav.webhooks")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "workspace" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenWorkspace}
          title={t("nav.workspace")}
        >
          <DevicesIcon size={17} />
          {!collapsed && t("nav.workspace")}
        </button>
        <button
          type="button"
          className={`sidebar-footer-btn${view === "settings" ? " sidebar-footer-btn--active" : ""}`}
          onClick={onOpenSettings}
          title={t("nav.settings")}
        >
          <SettingsIcon size={17} />
          {!collapsed && t("nav.settings")}
        </button>
      </div>
    </div>
  );
}

export default Sidebar;
