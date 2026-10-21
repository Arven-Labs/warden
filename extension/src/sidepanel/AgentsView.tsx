import { useState } from "react";
import ActivityView from "./ActivityView";
import AgentTasksView from "./AgentTasksView";
import GrantsSection from "./GrantsSection";
import OrgView from "./OrgView";
import UsageView from "./UsageView";
import { t } from "../i18n/index.ts";

/** P120, P123, P121 — the agents' organization, the work they hand to each other and the feed of what happened, side by side. A node of the
 * tree opens the agent's tasks (the filter here) or a new conversation with it (whoever assembles this screen decides how, because the chat belongs to another
 * tab). A feed event opens the agent's tasks, the conversation between two agents or its channel, the last two also through the chat. */
export default function AgentsView({
  onOpenChat,
  onOpenConversation,
  onOpenChannel,
}: {
  onOpenChat: (id: string) => void;
  onOpenConversation: (conversationId: string) => void;
  onOpenChannel: (agent: string) => void;
}) {
  const [part, setPart] = useState<"org" | "tasks" | "activity" | "usage">("org");
  const [taskAgent, setTaskAgent] = useState<string | null>(null);

  return (
    <div className="agents-view">
      <nav className="tab-bar tab-bar--sub">
        <button type="button" className={part === "org" ? "tab tab--active" : "tab"} onClick={() => setPart("org")}>
          {t("agentsTab.org")}
        </button>
        <button
          type="button"
          className={part === "tasks" ? "tab tab--active" : "tab"}
          onClick={() => {
            setTaskAgent(null);
            setPart("tasks");
          }}
        >
          {t("agentsTab.tasks")}
        </button>
        <button type="button" className={part === "activity" ? "tab tab--active" : "tab"} onClick={() => setPart("activity")}>
          {t("agentsTab.activity")}
        </button>
        <button type="button" className={part === "usage" ? "tab tab--active" : "tab"} onClick={() => setPart("usage")}>
          {t("agentsTab.usage")}
        </button>
      </nav>
      {part === "org" ? (
        <OrgView
          onOpenChat={onOpenChat}
          onOpenTasks={(id) => {
            setTaskAgent(id);
            setPart("tasks");
          }}
        />
      ) : part === "tasks" ? (
        <>
          <AgentTasksView agent={taskAgent} onClearAgent={() => setTaskAgent(null)} />
          <GrantsSection />
        </>
      ) : part === "usage" ? (
        <UsageView />
      ) : (
        <ActivityView
          onOpen={(to) => {
            if (to.kind === "tasks") {
              setTaskAgent(to.agent);
              setPart("tasks");
            } else if (to.kind === "conversation") {
              onOpenConversation(to.id);
            } else {
              onOpenChannel(to.agent);
            }
          }}
        />
      )}
    </div>
  );
}
