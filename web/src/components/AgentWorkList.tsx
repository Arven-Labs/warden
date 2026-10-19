import { workLabel, type AgentWorkItem } from "../hub/agentWork";
import { shortDate } from "../i18n/format";
import { t } from "../i18n";

interface Props {
  agent: string;
  items: AgentWorkItem[];
  /** Conversations with a turn waiting for its answer. */
  pendingIds: string[];
  onOpen: (conversationId: string) => void;
}

/** P121 — what an agent did outside its channel: the notes it traded with other agents and the runs nobody was watching. Opens each one
 * as the conversation it is. */
export default function AgentWorkList({ agent, items, pendingIds, onOpen }: Props) {
  if (items.length === 0) {
    return <p className="skills-hint agents-empty">{t("work.empty", { agent })}</p>;
  }
  return (
    <ul className="conversations-list work-list" aria-label={t("work.label", { agent })}>
      {items.map((item) => (
        <li key={item.id} className="conversation">
          <button type="button" className="conversation-open" onClick={() => onOpen(item.id)}>
            <span className="conversation-title">
              {item.title}
              <span className="work-kind"> · {workLabel(item)}</span>
            </span>
            <span className="conversation-meta">{pendingIds.includes(item.id) ? t("convs.answering") : shortDate(item.updatedAt)}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}
