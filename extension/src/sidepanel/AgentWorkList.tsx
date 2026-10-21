import { intlTag, t } from "../i18n/index.ts";
import { workLabel, type AgentWorkItem } from "./lib/agentWork";

interface Props {
  agent: string;
  items: AgentWorkItem[];
  /** Conversations with a turn waiting for its answer. */
  pendingIds: string[];
  onOpen: (conversationId: string) => void;
  onBack: () => void;
}

/** "14:05" today, "12/09" before that. */
function shortDate(millis: number): string {
  const date = new Date(millis);
  return date.toDateString() === new Date().toDateString()
    ? date.toLocaleTimeString(intlTag(), { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString(intlTag(), { day: "2-digit", month: "2-digit" });
}

/** P121 — what an agent did outside its channel: the notes it exchanged with other agents and the runs with nobody watching. Each one opens in the
 * chat as the conversation it is. */
export default function AgentWorkList({ agent, items, pendingIds, onOpen, onBack }: Props) {
  return (
    <div className="channels-view">
      <header className="thread-bar">
        <button type="button" className="link-button" onClick={onBack}>
          {t("app.backToChannels")}
        </button>
        <strong>{t("channels.workTitle", { agent })}</strong>
      </header>
      {items.length === 0 ? (
        <p className="hub-empty">{t("channels.workEmpty", { agent })}</p>
      ) : (
        <ul className="channel-list" aria-label={t("channels.workLabel", { agent })}>
          {items.map((item) => (
            <li key={item.id}>
              <button type="button" className="channel-row" onClick={() => onOpen(item.id)}>
                <span className="channel-name">
                  {item.title}
                  <span className="channel-meta"> · {workLabel(item)}</span>
                </span>
                <span className="channel-meta">{pendingIds.includes(item.id) ? t("channels.answering") : shortDate(item.updatedAt)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
