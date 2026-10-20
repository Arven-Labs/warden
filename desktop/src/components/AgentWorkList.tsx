import { t } from "../i18n";
import { shortDate } from "../i18n/format";
import { workLabel, type AgentWorkItem } from "../lib/agentWork";

interface AgentWorkListProps {
  agent: string;
  items: AgentWorkItem[];
  /** Conversations with a turn waiting for its answer. */
  answeringIds: string[];
  onOpen: (conversationId: string) => void;
  onBack: () => void;
}

/** P121 — what an agent did outside its channel: the notes it traded with other agents and the runs nobody was watching. Opens each one
 * as the conversation it is. */
function AgentWorkList({ agent, items, answeringIds, onOpen, onBack }: AgentWorkListProps) {
  return (
    <div className="chat-area">
      <div className="chat-header">
        <button type="button" className="chat-header-action" onClick={onBack}>
          {t("agentWork.back")}
        </button>
        <span className="chat-header-label">{t("agentWork.title", { agent })}</span>
      </div>
      {items.length === 0 ? (
        <p className="agent-work-empty">{t("agentWork.empty", { agent })}</p>
      ) : (
        <ul className="agent-work-list" aria-label={t("agentWork.list", { agent })}>
          {items.map((item) => (
            <li key={item.id}>
              <button type="button" className="agent-work-item" onClick={() => onOpen(item.id)}>
                <span className="agent-work-title">
                  {item.title}
                  <span className="agent-work-kind"> · {workLabel(item)}</span>
                </span>
                <span className="agent-work-meta">{answeringIds.includes(item.id) ? t("contacts.answering") : shortDate(item.updatedAt)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default AgentWorkList;
