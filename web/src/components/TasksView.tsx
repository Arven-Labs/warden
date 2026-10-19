import { useCallback, useEffect, useState } from "react";
import { TaskError, type ServerConnection } from "../hub/connection";
import type { Task, TaskInfo } from "../hub/messages";
import { intlTag, t, type Key } from "../i18n";
import { rich } from "../i18n/rich";

// Scheduled tasks (P92): a prompt an agent runs on its own, on the hub. The same list, create, edit,
// pause, remove and "run now" as `warden-server tasks` and the desktop's Tasks screen. Every change
// asks for the pairing key, like the API keys: a task runs an agent alone, with its tools and spend.
// Each run lands in the task's conversation (`task-<id>`), which "Open conversation" opens in the chat.

type ScheduleKind = "every" | "cron" | "once";

interface EditorState {
  /** The id being edited; absent for a new task. */
  originalId?: string;
  task: Task;
  kind: ScheduleKind;
  value: string;
}

type Change =
  | { kind: "save"; editor: EditorState }
  | { kind: "toggle"; task: TaskInfo }
  | { kind: "delete"; task: TaskInfo }
  | { kind: "run"; task: TaskInfo };

const dateFormatter = { format: (ms: number) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(ms) };
const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone;

const placeholders: Record<ScheduleKind, string> = { every: "1d", cron: "0 8 * * 1-5", once: "2026-10-01T09:00" };
const hints: Record<ScheduleKind, Key> = { every: "tasks.freqEvery", cron: "tasks.freqCron", once: "tasks.freqOnce" };

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function scheduleKind(task: Task): ScheduleKind {
  return task.cron ? "cron" : task.once ? "once" : "every";
}

function scheduleLabel(task: Task): string {
  const zone = task.timezone ? ` (${task.timezone})` : "";
  if (task.cron) return `cron ${task.cron}${zone}`;
  if (task.once) return `${t("tasks.onceAt", { when: task.once })}${zone}`;
  return t("tasks.everyN", { every: task.every ?? "?" });
}

function fromEditor(editor: EditorState): Task {
  const { every: _e, cron: _c, once: _o, ...rest } = editor.task;
  return { ...rest, [editor.kind]: editor.value.trim() };
}

function newEditor(): EditorState {
  return { task: { id: "", prompt: "", enabled: true, timezone: browserZone }, kind: "every", value: "" };
}

function editorFor(task: TaskInfo): EditorState {
  const kind = scheduleKind(task);
  const plain: Task = { id: task.id, agentId: task.agentId, prompt: task.prompt, timezone: task.timezone, enabled: task.enabled };
  return { originalId: task.id, task: plain, kind, value: task[kind] ?? "" };
}

function status(task: TaskInfo): string {
  if (task.scheduleError) return t("tasks.invalidSchedule", { error: task.scheduleError });
  const parts: string[] = [];
  if (task.running) parts.push(t("tasks.runningNow"));
  else if (task.lastRunAtMs) parts.push(`${t("tasks.last", { when: dateFormatter.format(task.lastRunAtMs) })}${task.lastError ? t("tasks.failed") : ""}`);
  else parts.push(t("tasks.neverRan"));
  if (!task.enabled) parts.push(t("tasks.paused"));
  else if (task.nextRunAtMs) parts.push(t("tasks.next", { when: dateFormatter.format(task.nextRunAtMs) }));
  else if (!task.running) parts.push(t("tasks.finished"));
  return parts.join(" · ");
}

export default function TasksView({ conn, onOpenConversation }: { conn: ServerConnection | null; onOpenConversation: (id: string) => void }) {
  const [tasks, setTasks] = useState<TaskInfo[] | null>(null);
  const [runsHere, setRunsHere] = useState(true);
  const [agentIds, setAgentIds] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [asking, setAsking] = useState<Change | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!conn) return;
    try {
      const list = await conn.listTasks();
      setTasks(list.tasks);
      setRunsHere(list.runsHere);
      setError(null);
    } catch (err) {
      setTasks((current) => current ?? []);
      setError(t("tasks.listFailed", { error: message(err) }));
    }
  }, [conn]);

  useEffect(() => {
    void refresh();
    if (!conn) return;
    conn
      .requestSettings()
      .then(({ settings }) => setAgentIds(settings.agents.map((a) => a.id)))
      .catch(() => setAgentIds([]));
    // A run finished (scheduled or "run now"): its status changed.
    return conn.onConversationsChanged((id) => {
      if (id.startsWith("task-")) void refresh();
    });
  }, [conn, refresh]);

  function cancelKey() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  async function confirm() {
    if (!conn || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      const list =
        asking.kind === "save"
          ? await conn.saveTask(pairingKey, fromEditor(asking.editor), asking.editor.originalId)
          : asking.kind === "toggle"
            ? await conn.setTaskEnabled(pairingKey, asking.task.id, !asking.task.enabled)
            : asking.kind === "delete"
              ? await conn.deleteTask(pairingKey, asking.task.id)
              : await conn.runTask(pairingKey, asking.task.id);
      setTasks(list.tasks);
      setRunsHere(list.runsHere);
      if (asking.kind === "save") setEditor(null);
      setConfirmDelete(null);
      setError(null);
      cancelKey();
    } catch (err) {
      if (err instanceof TaskError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setError(message(err));
      }
    } finally {
      setBusy(false);
    }
  }

  function ask(change: Change) {
    setError(null);
    setKeyError(null);
    setAsking(change);
  }

  const labels: Record<Change["kind"], string> = { save: t("tasks.saveTask"), toggle: t("common.confirm"), delete: t("common.delete"), run: t("tasks.runNow") };
  const keyPrompt = asking && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("pairingKey.hint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : labels[asking.kind]}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  function update(patch: Partial<EditorState>) {
    setEditor((current) => (current ? { ...current, ...patch } : current));
  }

  function updateTask(patch: Partial<Task>) {
    setEditor((current) => (current ? { ...current, task: { ...current.task, ...patch } } : current));
  }

  if (editor) {
    return (
      <form
        className="skills-editor"
        onSubmit={(e) => {
          e.preventDefault();
          ask({ kind: "save", editor });
        }}
      >
        <strong>{editor.originalId ? t("tasks.editing", { id: editor.originalId }) : t("tasks.new")}</strong>
        <label>
          {t("common.name")}
          <input value={editor.task.id} onChange={(e) => updateTask({ id: e.target.value })} placeholder={t("tasks.namePlaceholder")} required />
          <span className="skills-hint">{t("tasks.nameHint")}</span>
        </label>
        <label>
          {t("tasks.agent")}
          <select value={editor.task.agentId ?? ""} onChange={(e) => updateTask({ agentId: e.target.value || undefined })}>
            <option value="">{t("tasks.noAgent")}</option>
            {agentIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t("tasks.prompt")}
          <textarea
            value={editor.task.prompt}
            onChange={(e) => updateTask({ prompt: e.target.value })}
            rows={6}
            placeholder={t("tasks.promptPlaceholder")}
            required
          />
        </label>
        <label>
          {t("tasks.when")}
          <select value={editor.kind} onChange={(e) => update({ kind: e.target.value as ScheduleKind, value: "" })}>
            <option value="every">{t("tasks.kindEvery")}</option>
            <option value="cron">{t("tasks.kindCron")}</option>
            <option value="once">{t("tasks.kindOnce")}</option>
          </select>
        </label>
        <label>
          {editor.kind === "every" ? t("tasks.valueEvery") : editor.kind === "cron" ? t("tasks.valueCron") : t("tasks.valueOnce")}
          <input value={editor.value} onChange={(e) => update({ value: e.target.value })} placeholder={placeholders[editor.kind]} required />
          <span className="skills-hint">{t(hints[editor.kind])}</span>
        </label>
        {editor.kind !== "every" && (
          <label>
            {t("tasks.timezone")}
            <input value={editor.task.timezone ?? ""} onChange={(e) => updateTask({ timezone: e.target.value || undefined })} placeholder={browserZone} />
            <span className="skills-hint">{t("tasks.timezoneHint")}</span>
          </label>
        )}
        {error && <p className="error-banner">{error}</p>}
        {asking ? (
          keyPrompt
        ) : (
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={!conn}>
              {t("common.save")}
            </button>
            <button
              type="button"
              className="link-button"
              onClick={() => {
                setEditor(null);
                setError(null);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        )}
      </form>
    );
  }

  return (
    <div className="skills-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("tasks.intro")}</span>
        <button
          type="button"
          className="primary-button"
          onClick={() => {
            setError(null);
            setEditor(newEditor());
          }}
        >
          {t("tasks.newButton")}
        </button>
      </div>
      {!runsHere && <p className="skills-hint">{rich("tasks.notHere", { cmd: <code>warden-server serve --run-tasks</code> })}</p>}
      {error && <p className="error-banner">{error}</p>}
      {asking && keyPrompt}
      {tasks === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : tasks.length === 0 ? (
        <p className="skills-hint">{t("tasks.none")}</p>
      ) : (
        <ul className="skills-list">
          {tasks.map((task) => (
            <li key={task.id} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{task.id}</span>
                {confirmDelete === task.id ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => ask({ kind: "delete", task })}>
                      {t("common.delete")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      {t("common.keep")}
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button type="button" className="link-button" onClick={() => onOpenConversation(`task-${task.id}`)}>
                      {t("tasks.openConversation")}
                    </button>
                    <button type="button" className="link-button" disabled={task.running} onClick={() => ask({ kind: "run", task })}>
                      {t("tasks.runNow")}
                    </button>
                    <button type="button" className="link-button" onClick={() => ask({ kind: "toggle", task })}>
                      {task.enabled ? t("tasks.pause") : t("tasks.resume")}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor(editorFor(task));
                      }}
                    >
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(task.id)}>
                      {t("common.delete")}
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">
                {scheduleLabel(task)} · {task.agentId ? t("tasks.withAgent", { agent: task.agentId }) : t("tasks.withoutAgent")}
              </p>
              <p className="skills-hint">{status(task)}</p>
              {task.lastError && <p className="skills-hint">{t("common.error", { error: task.lastError })}</p>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
