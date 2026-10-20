import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";
import type { AgentEntry, RunMessage, Task, TaskInfo, TaskList } from "../types";
import { hubTasks } from "../lib/hub";
import { KeyCancelled, usePairingKey } from "./PairingKeyDialog";

/** Where the tasks live: this computer's `config.toml`, or the hub in use. The hub's changes ask for its pairing key. */
interface TaskApi {
  list: () => Promise<TaskList>;
  save: (originalId: string | null, task: Task) => Promise<TaskList>;
  setEnabled: (id: string, enabled: boolean) => Promise<TaskList>;
  remove: (id: string) => Promise<TaskList>;
  run: (id: string) => Promise<TaskList>;
  history: (id: string) => Promise<RunMessage[]>;
}

const localTasks: TaskApi = {
  list: () => invoke<TaskList>("list_tasks"),
  save: (originalId, task) => invoke<TaskList>("save_task", { originalId, task }),
  setEnabled: (id, enabled) => invoke<TaskList>("set_task_enabled_cmd", { id, enabled }),
  remove: (id) => invoke<TaskList>("delete_task", { id }),
  run: (id) => invoke<TaskList>("run_task_now", { id }),
  history: (id) => invoke<RunMessage[]>("task_history", { id }),
};

type ScheduleKind = "every" | "cron" | "once";

interface EditorState {
  originalId?: string;
  task: Task;
  kind: ScheduleKind;
  value: string;
}

const dateFormatter = () => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" });
const localZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

const placeholders: Record<ScheduleKind, string> = { every: "1d", cron: "0 8 * * 1-5", once: "2026-10-01T09:00" };
const hint = (kind: ScheduleKind): string => t(kind === "every" ? "tasks.everyHint" : kind === "cron" ? "tasks.cronHint" : "tasks.onceHint");

function scheduleLabel(task: Task): string {
  const zone = task.timezone ? ` (${task.timezone})` : "";
  if (task.cron) return t("tasks.cron", { cron: task.cron, zone });
  if (task.once) return t("tasks.once", { once: task.once, zone });
  return t("tasks.every", { every: task.every ?? "?" });
}

function statusLine(task: TaskInfo): string {
  if (task.scheduleError) return t("tasks.invalid", { error: task.scheduleError });
  const parts: string[] = [];
  if (task.running) parts.push(t("tasks.runningNow"));
  else if (task.lastRunAtMs) parts.push(t(task.lastError ? "tasks.lastRunFailed" : "tasks.lastRun", { date: dateFormatter().format(task.lastRunAtMs) }));
  else parts.push(t("tasks.neverRan"));
  if (!task.enabled) parts.push(t("tasks.paused"));
  else if (task.nextRunAtMs) parts.push(t("tasks.next", { date: dateFormatter().format(task.nextRunAtMs) }));
  else if (!task.running) parts.push(t("tasks.done"));
  return parts.join(" · ");
}

function fromEditor(editor: EditorState): Task {
  const { every: _e, cron: _c, once: _o, ...rest } = editor.task;
  return { ...rest, [editor.kind]: editor.value.trim() };
}

function editorFor(task: TaskInfo): EditorState {
  const kind: ScheduleKind = task.cron ? "cron" : task.once ? "once" : "every";
  const plain: Task = { id: task.id, agentId: task.agentId, prompt: task.prompt, timezone: task.timezone, enabled: task.enabled };
  return { originalId: task.id, task: plain, kind, value: task[kind] ?? "" };
}

/** A task's conversation on this machine, read-only: the last answer, and the rest on request. */
function TaskHistory({ id, refreshKey, api }: { id: string; refreshKey: number; api: TaskApi }) {
  const [messages, setMessages] = useState<RunMessage[] | null>(null);
  const [all, setAll] = useState(false);

  useEffect(() => {
    api
      .history(id)
      .then(setMessages)
      .catch(() => setMessages([]));
  }, [id, refreshKey, api]);

  if (messages === null) return <p className="settings-hint">{t("common.loading")}</p>;
  if (messages.length === 0) return <p className="settings-hint">{t("tasks.noRuns")}</p>;
  const shown = all ? messages : messages.filter((m) => m.role === "assistant").slice(-1);
  return (
    <div className="task-history">
      {shown.map((m, i) => (
        <div key={i} className={`task-history-message task-history-message--${m.role}`}>
          <span className="settings-hint">
            {m.role === "user" ? t("tasks.asked") : t("tasks.answer")} · {dateFormatter().format(m.createdAt)}
          </span>
          <p className="task-history-content">{m.content}</p>
        </div>
      ))}
      {messages.length > 1 && (
        <button type="button" className="settings-browse-btn" onClick={() => setAll(!all)}>
          {all ? t("tasks.onlyLast") : t("tasks.wholeHistory", { count: messages.length })}
        </button>
      )}
    </div>
  );
}

/**
 * P92 — scheduled tasks: a prompt an agent runs on its own. The same list, create, edit, pause,
 * remove and "run now" as `warden-server tasks` and the web's Tasks tab, on this machine's
 * `config.toml` (which syncs). Whether this computer runs them on schedule is a switch of its own.
 */
function TasksView({ agents, remote = false }: { agents: AgentEntry[]; remote?: boolean }) {
  const { askKey, dialog } = usePairingKey();
  const api = useMemo(() => (remote ? hubTasks(askKey) : localTasks), [remote, askKey]);
  const [list, setList] = useState<TaskList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  const load = useCallback(() => {
    api
      .list()
      .then((next) => {
        setList(next);
        setRefreshKey((k) => k + 1);
      })
      .catch((err) => setError(String(err)));
  }, [api]);

  useEffect(load, [load]);

  // While a run is going, check back until it's done.
  const anyRunning = list?.tasks.some((t) => t.running) ?? false;
  useEffect(() => {
    if (!anyRunning) return;
    const timer = window.setInterval(load, 3000);
    return () => window.clearInterval(timer);
  }, [anyRunning, load]);

  /** Runs one change and shows the list it answers with. False when it failed, or when the person declined to give the key. */
  async function act(change: () => Promise<TaskList>): Promise<boolean> {
    setError(null);
    try {
      setList(await change());
      setRefreshKey((k) => k + 1);
      return true;
    } catch (err) {
      if (!(err instanceof KeyCancelled)) setError(String(err));
      return false;
    }
  }

  async function handleSave() {
    if (!editor) return;
    setSaving(true);
    const saved = await act(() => api.save(editor.originalId ?? null, fromEditor(editor)));
    setSaving(false);
    if (saved) setEditor(null);
  }

  function update(patch: Partial<EditorState>) {
    setEditor((current) => (current ? { ...current, ...patch } : current));
  }

  function updateTask(patch: Partial<Task>) {
    setEditor((current) => (current ? { ...current, task: { ...current.task, ...patch } } : current));
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("tasks.title")}</h2>
      <p className="settings-hint">{t("tasks.hint")}</p>
      {error && <p className="settings-error-banner">{error}</p>}

      {remote ? (
        list && <p className="settings-hint">{list.runHere ? t("tasks.hubRuns") : t("tasks.hubNotRuns")}</p>
      ) : (
        <section className="settings-section">
          <label className="settings-field settings-checkbox-field">
            <span className="settings-checkbox-row">
              <input
                type="checkbox"
                checked={list?.runHere ?? false}
                disabled={list === null}
                onChange={(e) => {
                  const enabled = e.currentTarget.checked;
                  void act(() => invoke<TaskList>("set_run_tasks_here", { enabled }));
                }}
              />
              <span className="settings-label">{t("tasks.runHere")}</span>
            </span>
            <span className="settings-hint">
              {rich("tasks.runHereHint", { command: <code>warden-server serve --run-tasks</code> })}
              {list?.runHere && !list.hubRunning && t("tasks.hubOff")}
            </span>
          </label>
        </section>
      )}

      {editor && (
        <div className="provider-card skill-editor">
          <span className="settings-section-title">{editor.originalId ? t("tasks.editTitle", { id: editor.originalId }) : t("tasks.newTask")}</span>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.name")}</span>
            <input className="settings-input" type="text" placeholder="daily-summary" value={editor.task.id} onChange={(e) => updateTask({ id: e.currentTarget.value })} />
            <span className="settings-hint">{t("tasks.nameHint")}</span>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.agent")}</span>
            <select className="settings-select" value={editor.task.agentId ?? ""} onChange={(e) => updateTask({ agentId: e.currentTarget.value || undefined })}>
              <option value="">{t("tasks.noAgent")}</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.id}
                </option>
              ))}
            </select>
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.prompt")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={6}
              placeholder={t("tasks.promptPlaceholder")}
              value={editor.task.prompt}
              onChange={(e) => updateTask({ prompt: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("tasks.when")}</span>
            <select className="settings-select" value={editor.kind} onChange={(e) => update({ kind: e.currentTarget.value as ScheduleKind, value: "" })}>
              <option value="every">{t("tasks.whenEvery")}</option>
              <option value="cron">{t("tasks.whenCron")}</option>
              <option value="once">{t("tasks.whenOnce")}</option>
            </select>
          </label>

          <label className="settings-field">
            <span className="settings-label">{editor.kind === "every" ? t("tasks.interval") : editor.kind === "cron" ? t("tasks.cronExpr") : t("tasks.dateTime")}</span>
            <input className="settings-input" type="text" placeholder={placeholders[editor.kind]} value={editor.value} onChange={(e) => update({ value: e.currentTarget.value })} />
            <span className="settings-hint">{hint(editor.kind)}</span>
          </label>

          {editor.kind !== "every" && (
            <label className="settings-field">
              <span className="settings-label">{t("tasks.zone")}</span>
              <input
                className="settings-input"
                type="text"
                placeholder={localZone}
                value={editor.task.timezone ?? ""}
                onChange={(e) => updateTask({ timezone: e.currentTarget.value || undefined })}
              />
              <span className="settings-hint">{t("tasks.zoneHint")}</span>
            </label>
          )}

          <div className="skill-editor-actions">
            <button type="button" className="settings-save-btn" onClick={() => void handleSave()} disabled={saving}>
              {saving ? t("common.saving") : t("tasks.save")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => setEditor(null)} disabled={saving}>
              {t("common.cancel")}
            </button>
          </div>
        </div>
      )}

      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("tasks.yours")}</h3>
          <button
            type="button"
            className="settings-browse-btn"
            onClick={() => setEditor({ task: { id: "", prompt: "", enabled: true, timezone: localZone }, kind: "every", value: "" })}
          >
            {t("tasks.add")}
          </button>
        </div>

        {list === null ? (
          <p className="settings-hint">{t("tasks.loading")}</p>
        ) : list.tasks.length === 0 ? (
          <p className="settings-hint">{t("tasks.none")}</p>
        ) : (
          <div className="provider-list">
            {list.tasks.map((task) => (
              <div className="provider-card skill-card" key={task.id}>
                <div className="skill-card-header">
                  <span className="skill-card-name">{task.id}</span>
                  <div className="skill-card-actions">
                    {confirmDelete === task.id ? (
                      <>
                        <span className="settings-hint">{t("tasks.confirmDelete")}</span>
                        <button type="button" className="provider-delete-btn" onClick={() => void act(() => api.remove(task.id)).then(() => setConfirmDelete(null))}>
                          {t("common.delete")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setConfirmDelete(null)}>
                          {t("common.keep")}
                        </button>
                      </>
                    ) : (
                      <>
                        <button type="button" className="settings-browse-btn" onClick={() => setExpanded(expanded === task.id ? null : task.id)}>
                          {expanded === task.id ? t("tasks.hideResult") : t("tasks.lastResult")}
                        </button>
                        <button type="button" className="settings-browse-btn" disabled={task.running} onClick={() => void act(() => api.run(task.id))}>
                          {t("tasks.runNow")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => void act(() => api.setEnabled(task.id, !task.enabled))}>
                          {task.enabled ? t("tasks.pause") : t("tasks.resume")}
                        </button>
                        <button type="button" className="settings-browse-btn" onClick={() => setEditor(editorFor(task))}>
                          {t("common.edit")}
                        </button>
                        <button
                          type="button"
                          className="provider-delete-btn"
                          onClick={() => setConfirmDelete(task.id)}
                          aria-label={t("tasks.deleteLabel", { id: task.id })}
                          title={t("tasks.deleteTitle")}
                        >
                          🗑
                        </button>
                      </>
                    )}
                  </div>
                </div>
                <p className="skill-card-description">
                  {scheduleLabel(task)} · {task.agentId ? t("tasks.withAgent", { id: task.agentId }) : t("tasks.withoutAgent")}
                </p>
                <p className="settings-hint">{statusLine(task)}</p>
                {task.lastError && <p className="settings-hint">{t("tasks.error", { error: task.lastError })}</p>}
                {expanded === task.id && <TaskHistory id={task.id} refreshKey={refreshKey} api={api} />}
              </div>
            ))}
          </div>
        )}
      </section>
      {dialog}
    </div>
  );
}

export default TasksView;
