import { useCallback, useEffect, useMemo, useState } from "react";
import type { AgentTask, AgentTaskAction } from "../protocol/messages";
import type { AgentTasksResponse } from "../background/popup_protocol";
import { ACTION_LABEL, actionsFor, durationLabel, formatTokens, groupTasks, involvingAgent, STATE_LABEL, STATE_MARK, type TaskGroup } from "./lib/agentTasks";
import { intlTag, t } from "../i18n/index.ts";
import { rich } from "../i18n/react.tsx";
import PairingKeyForm from "./PairingKeyForm";

const REFRESH_MS = 3000;
const dateFormatter = { format: (ms: number) => new Intl.DateTimeFormat(intlTag(), { dateStyle: "short", timeStyle: "short" }).format(ms) };

function GroupHeader({ group }: { group: TaskGroup }) {
  const { counts } = group;
  const parts = [
    counts.done > 0 && t(counts.done > 1 ? "tally.doneMany" : "tally.doneOne", { count: counts.done }),
    counts.running > 0 && t("tally.active", { count: counts.running }),
    counts.waiting > 0 && t("tasksView.waiting", { count: counts.waiting }),
    counts.paused > 0 && t(counts.paused > 1 ? "tasksView.pausedMany" : "tasksView.pausedOne", { count: counts.paused }),
    counts.pending > 0 && t(counts.pending > 1 ? "tasksView.pendingMany" : "tasksView.pendingOne", { count: counts.pending }),
    counts.failed > 0 && t("tally.failed", { count: counts.failed }),
    counts.cancelled > 0 && t(counts.cancelled > 1 ? "tally.cancelledMany" : "tally.cancelledOne", { count: counts.cancelled }),
  ].filter(Boolean);
  return (
    <div className="agent-work-header">
      <div className="agent-work-title">
        <strong>{group.owner ?? t("tasksView.anAgent")}</strong>
        <span className="skills-hint">{dateFormatter.format(group.createdAtMs)}</span>
        {group.active && <span className="agent-work-live">{t("tasksView.working")}</span>}
      </div>
      <div
        className="agent-work-bar"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={group.percent}
        aria-label={t("tasksView.progressLabel", { finished: group.finished, total: group.total })}
      >
        <div className="agent-work-bar-fill" style={{ width: `${group.percent}%` }} />
      </div>
      <div className="skills-hint">
        {t("tasksView.progress", { finished: group.finished, total: group.total, percent: group.percent })} · {parts.join(", ")}
        {group.totalTokens > 0 && ` · ${t("tally.tokens", { count: formatTokens(group.totalTokens) })}`}
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
    <li className={`agent-work-task agent-work-task--${task.state}`} style={depth > 0 ? { marginLeft: `${depth * 1.1}em` } : undefined}>
      <button type="button" className="agent-work-task-line" onClick={() => setOpen((o) => !o)} aria-expanded={open}>
        <span className="agent-work-mark" title={STATE_LABEL[task.state]} aria-label={STATE_LABEL[task.state]}>
          {STATE_MARK[task.state]}
        </span>
        <span className="agent-work-assignee">{task.assignee}</span>
        <span className="agent-work-meta">
          {task.model && <span className="org-badge">{task.model}</span>}
          {duration && <span>{duration}</span>}
          {task.totalTokens != null && <span>{t("tasksView.tok", { count: formatTokens(task.totalTokens) })}</span>}
        </span>
        <span className="agent-work-objective">{task.objective}</span>
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
          <p className="skills-hint">{t("tasksView.task")}</p>
          <pre className="approval-detail">{task.objective}</pre>
          {detail ? (
            <>
              <p className="skills-hint">{task.state === "done" ? t("tasksView.result") : t("tasksView.whyStopped")}</p>
              <pre className="approval-detail">{detail}</pre>
            </>
          ) : (
            <p className="skills-hint">{t("tasksView.noResult", { state: STATE_LABEL[task.state] })}</p>
          )}
        </div>
      )}
    </li>
  );
}

/** P123 — the work the agents handed to each other in the background: who does what, how far each manager's batch got and what
 * it cost. Updates while the tab is open. A task still running on the hub can be paused, resumed or stopped (along with its subtasks), and
 * that asks for the pairing key, like every change on the hub. `agent` filters by what an agent received and delegated. */
export default function AgentTasksView({ agent, onClearAgent }: { agent: string | null; onClearAgent: () => void }) {
  const [tasks, setTasks] = useState<AgentTask[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [asking, setAsking] = useState<{ task: AgentTask; action: AgentTaskAction } | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    chrome.runtime.sendMessage({ type: "listAgentTasks" }).then((res: AgentTasksResponse) => {
      if (res.ok) {
        setTasks(res.tasks);
        setError(null);
      } else {
        setError(res.error ?? t("tasksView.listFailed"));
      }
      setNow(Date.now());
    });
  }, []);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  function cancelKey() {
    setAsking(null);
    setKeyError(null);
  }

  function confirm(pairingKey: string) {
    if (!asking) return;
    setBusy(true);
    setKeyError(null);
    chrome.runtime.sendMessage({ type: "controlAgentTask", pairingKey, taskId: asking.task.id, action: asking.action }).then((res: AgentTasksResponse) => {
      setBusy(false);
      if (res.ok) {
        setTasks(res.tasks);
        setError(null);
        cancelKey();
      } else if (res.authRejected) {
        setKeyError(t("pairing.wrong"));
      } else {
        cancelKey();
        setError(res.error ?? t("tasksView.controlFailed"));
      }
    });
  }

  const groups = useMemo(() => groupTasks(agent ? involvingAgent(tasks ?? [], agent) : (tasks ?? [])), [tasks, agent]);

  return (
    <div className="agents-pane">
      <p className="skills-hint">{t("tasksView.intro")}</p>
      {agent && (
        <p className="skills-hint">
          {rich("tasksView.onlyOf", { agent: <strong>{agent}</strong> })}{" "}
          <button type="button" className="link-button" onClick={onClearAgent}>
            {t("tasksView.showAll")}
          </button>
        </p>
      )}
      {error && <p className="error-banner">{error}</p>}
      {asking && (
        <PairingKeyForm busy={busy} error={keyError} onConfirm={confirm} onCancel={cancelKey}>
          {rich("tasksView.actionOn", {
            action: ACTION_LABEL[asking.action],
            who: <strong>{asking.task.assignee}</strong>,
            subtasks: asking.action === "cancel" ? t("tasksView.andSubtasks") : "",
          })}
        </PairingKeyForm>
      )}
      {tasks === null && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {tasks !== null && groups.length === 0 && <p className="skills-hint">{agent ? t("tasksView.noneAbout", { agent }) : t("tasksView.none")}</p>}
      {groups.map((group) => (
        <section key={group.group} className="agent-work-group">
          <GroupHeader group={group} />
          <ul className="agent-work-list">
            {group.rows.map(({ task, depth }) => (
              <TaskItem
                key={task.id}
                task={task}
                depth={depth}
                now={now}
                onAsk={(item, action) => {
                  setError(null);
                  setKeyError(null);
                  setAsking({ task: item, action });
                }}
              />
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}
