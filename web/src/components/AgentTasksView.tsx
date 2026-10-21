import { useCallback, useEffect, useMemo, useState } from "react";
import { TaskError, type ServerConnection } from "../hub/connection";
import type { AgentTask, AgentTaskAction } from "../hub/messages";
import { ACTION_LABEL, actionsFor, durationLabel, formatTokens, groupTasks, involvingAgent, STATE_LABEL, STATE_MARK, type TaskGroup } from "../hub/agentTasks";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";

const REFRESH_MS = 3000;
const dateFormatter = { format: (ms: number) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(ms) };

function GroupHeader({ group }: { group: TaskGroup }) {
  const { counts } = group;
  const parts = [
    counts.done > 0 && t(counts.done > 1 ? "tally.doneMany" : "tally.doneOne", { count: counts.done }),
    counts.running > 0 && t("tally.active", { count: counts.running }),
    counts.waiting > 0 && t("agentTasks.waiting", { count: counts.waiting }),
    counts.paused > 0 && t(counts.paused > 1 ? "agentTasks.pausedMany" : "agentTasks.pausedOne", { count: counts.paused }),
    counts.pending > 0 && t(counts.pending > 1 ? "agentTasks.pendingMany" : "agentTasks.pendingOne", { count: counts.pending }),
    counts.failed > 0 && t("tally.failed", { count: counts.failed }),
    counts.cancelled > 0 && t(counts.cancelled > 1 ? "tally.cancelledMany" : "tally.cancelledOne", { count: counts.cancelled }),
  ].filter(Boolean);
  return (
    <div className="agent-work-header">
      <div className="agent-work-title">
        <strong>{group.owner ?? t("agentTasks.anAgent")}</strong>
        <span className="skills-hint">{dateFormatter.format(group.createdAtMs)}</span>
        {group.active && <span className="agent-work-live">{t("agentTasks.working")}</span>}
      </div>
      <div
        className="agent-work-bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={group.percent}
        aria-label={t("agentTasks.progressLabel", { finished: group.finished, total: group.total })}
      >
        <div className="agent-work-bar-fill" style={{ width: `${group.percent}%` }} />
      </div>
      <div className="skills-hint">
        {t("agentTasks.progress", { finished: group.finished, total: group.total, percent: group.percent })} · {parts.join(", ")}
        {group.totalTokens > 0 && ` · ${t("agentTasks.tokens", { count: formatTokens(group.totalTokens) })}`}
      </div>
    </div>
  );
}

function TaskItem({ task, depth, now, onAsk }: { task: AgentTask; depth: number; now: number; onAsk: (task: AgentTask, action: AgentTaskAction) => void }) {
  const [open, setOpen] = useState(false);
  const detail = task.state === "done" ? task.result : task.error;
  const duration = durationLabel(task, now);
  const actions = actionsFor(task);
  return (
    <li className={`agent-work-task agent-work-task--${task.state}`} style={depth > 0 ? { marginLeft: `${depth * 1.4}em` } : undefined}>
      <button type="button" className="agent-work-task-line" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span className="agent-work-mark" title={STATE_LABEL[task.state]} aria-label={STATE_LABEL[task.state]}>
          {STATE_MARK[task.state]}
        </span>
        <span className="agent-work-assignee">{task.assignee}</span>
        <span className="agent-work-objective">{task.objective}</span>
        <span className="agent-work-meta">
          {task.model && <span className="org-badge">{task.model}</span>}
          {duration && <span>{duration}</span>}
          {task.totalTokens != null && <span>{t("agentTasks.tok", { count: formatTokens(task.totalTokens) })}</span>}
        </span>
      </button>
      {actions.length > 0 && (
        <div className="agent-work-actions">
          {actions.map((action) => (
            <button key={action} type="button" className="link-button" onClick={() => onAsk(task, action)}>
              {ACTION_LABEL[action]}
            </button>
          ))}
        </div>
      )}
      {open && (
        <div className="agent-work-detail">
          <p className="skills-hint">{t("agentTasks.task")}</p>
          <pre className="approval-detail">{task.objective}</pre>
          {detail ? (
            <>
              <p className="skills-hint">{task.state === "done" ? t("agentTasks.result") : t("agentTasks.whyStopped")}</p>
              <pre className="approval-detail">{detail}</pre>
            </>
          ) : (
            <p className="skills-hint">{t("agentTasks.noResult", { state: STATE_LABEL[task.state] })}</p>
          )}
        </div>
      )}
    </li>
  );
}

/** P123 — the work the agents handed to each other in the background: who does what, how far each manager's batch got
 * and what it cost. Updates while the tab is open. A task still running on the hub can be paused, resumed or
 * stopped (along with its subtasks), and that asks for the pairing key, like every change on the hub. */
export default function AgentTasksView({ conn, agent = null, onClearAgent }: { conn: ServerConnection | null; agent?: string | null; onClearAgent?: () => void }) {
  const [tasks, setTasks] = useState<AgentTask[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [asking, setAsking] = useState<{ task: AgentTask; action: AgentTaskAction } | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      setTasks(await conn.listAgentTasks());
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
    setNow(Date.now());
  }, [conn]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

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
      setTasks(await conn.controlAgentTask(pairingKey, asking.task.id, asking.action));
      setError(null);
      cancelKey();
    } catch (err) {
      if (err instanceof TaskError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        cancelKey();
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      setBusy(false);
    }
  }

  const keyPrompt = asking && (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      <p>
        {rich("agentTasks.actionOn", {
          action: ACTION_LABEL[asking.action],
          who: <strong>{asking.task.assignee}</strong>,
          subtasks: asking.action === "cancel" ? t("agentTasks.andSubtasks") : "",
        })}
      </p>
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("pairingKey.hint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className="primary-button" disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : t("common.confirm")}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancelKey}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  const groups = useMemo(() => groupTasks(agent ? involvingAgent(tasks ?? [], agent) : (tasks ?? [])), [tasks, agent]);

  return (
    <div className="usage-view">
      <p className="skills-hint">{t("agentTasks.intro")}</p>
      {agent && (
        <p className="skills-hint">
          {rich("agentTasks.onlyOf", { agent: <strong>{agent}</strong> })}{" "}
          {onClearAgent && (
            <button type="button" className="link-button" onClick={onClearAgent}>
              {t("agentTasks.showAll")}
            </button>
          )}
        </p>
      )}
      {error && <p className="error-banner">{error}</p>}
      {keyPrompt}
      {tasks === null && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {tasks !== null && groups.length === 0 && <p className="skills-hint">{agent ? t("agentTasks.noneAbout", { agent }) : t("agentTasks.none")}</p>}
      {groups.map((group) => (
        <section key={group.group} className="agent-work-group">
          <GroupHeader group={group} />
          <ul className="agent-work-list">
            {group.rows.map(({ task, depth }) => (
              <TaskItem key={task.id} task={task} depth={depth} now={now} onAsk={(item, action) => { setError(null); setKeyError(null); setAsking({ task: item, action }); }} />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
