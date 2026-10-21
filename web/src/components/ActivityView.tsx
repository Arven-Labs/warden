import { useCallback, useEffect, useMemo, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { ActivityEvent } from "../hub/messages";
import { agentsIn, destination, groupByDay, headline, involving, mark, type Destination } from "../hub/activity";
import { intlTag, t } from "../i18n";

const REFRESH_MS = 3000;
const timeFormatter = { format: (ms: number) => new Intl.DateTimeFormat(intlTag(), { timeStyle: "short" }).format(ms) };

/** P121 — the activity feed: who did what among the agents, newest to oldest, without opening each conversation. Updates while the
 * tab is open. A click goes to what the event touches: the agent's tasks, the conversation between two agents or its channel. */
export default function ActivityView({ conn, onOpen }: { conn: ServerConnection | null; onOpen: (to: Destination) => void }) {
  const [events, setEvents] = useState<ActivityEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [agent, setAgent] = useState("");

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      setEvents(await conn.listActivity());
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
    const timer = window.setInterval(() => void load(), REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  const agents = useMemo(() => agentsIn(events ?? []), [events]);
  // The filter names an agent that may leave the feed (its events aged out): then it shows everyone again.
  const chosen = agents.includes(agent) ? agent : "";
  const groups = useMemo(() => groupByDay(chosen ? involving(events ?? [], chosen) : (events ?? []), Date.now()), [events, chosen]);

  return (
    <div className="usage-view">
      <p className="skills-hint">{t("activityView.intro")}</p>
      <label className="settings-field activity-filter">
        {t("activityView.agent")}
        <select value={chosen} onChange={(e) => setAgent(e.target.value)}>
          <option value="">{t("activityView.all")}</option>
          {agents.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
      </label>
      {error && <p className="error-banner">{error}</p>}
      {events === null && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {events !== null && groups.length === 0 && (
        <p className="skills-hint">{chosen ? t("activityView.nothingAbout", { agent: chosen }) : t("activityView.nothing")}</p>
      )}
      {groups.map((group) => (
        <section key={group.label} className="activity-day">
          <h3 className="activity-day-title">{group.label}</h3>
          <ul className="activity-list">
            {group.events.map((event) => {
              const to = destination(event);
              return (
                <li key={event.id} className={`activity-item activity-item--${event.kind}`}>
                  <span className="activity-time">{timeFormatter.format(event.atMs)}</span>
                  <span className="activity-mark" aria-hidden="true">
                    {mark(event.kind)}
                  </span>
                  <div className="activity-body">
                    {to ? (
                      <button type="button" className="link-button activity-headline" onClick={() => onOpen(to)}>
                        {headline(event)}
                      </button>
                    ) : (
                      <span className="activity-headline">{headline(event)}</span>
                    )}
                    {event.text && <p className="activity-text">{event.text}</p>}
                  </div>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
