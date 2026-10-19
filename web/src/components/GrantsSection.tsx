import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { ConversationSummary, Grant, GrantsView } from "../hub/messages";
import { agoLabel } from "../hub/agentTasks";
import { t, type Key } from "../i18n";

const CATEGORY_KEY: Record<string, Key> = {
  delete_data: "grants.cat.delete_data",
  spend_money: "grants.cat.spend_money",
  critical_infra: "grants.cat.critical_infra",
  external_message: "grants.cat.external_message",
  publish_code: "grants.cat.publish_code",
  important_config: "grants.cat.important_config",
  elevated_agent: "grants.cat.elevated_agent",
};

/** P122 — o que a pessoa disse "sempre" nesta conexão: uma categoria de risco ou uma ferramenta, por conversa e agente. Guardado em arquivo,
 * vence depois dos dias que o hub tem configurados sem uso; senão vale só até a conexão fechar. Aqui dá para ver e tirar um, e o agente
 * volta a perguntar. */
export default function GrantsSection({ conn, conversations }: { conn: ServerConnection | null; conversations: ConversationSummary[] }) {
  const [view, setView] = useState<GrantsView | null>(null);
  const grants = view?.grants ?? null;
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      setView(await conn.listGrants());
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
  }, [load]);

  async function revoke(grant: Grant) {
    if (!conn) return;
    try {
      setView(await conn.revokeGrant(grant));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  const titleOf = (id: string) => conversations.find((c) => c.id === id)?.title || id;

  return (
    <section className="grants-section">
      <h3>{t("grants.title")}</h3>
      <p className="skills-hint">
        {t("grants.scope")}{" "}
        {view?.ttlDays !== undefined
          ? t("grants.kept", { days: view.ttlDays, unit: t(view.ttlDays === 1 ? "grants.day" : "grants.days") })
          : t("grants.session")}{" "}
        {t("grants.takeBack")}
      </p>
      {error && <p className="error-banner">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="skills-hint">{t("grants.none")}</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{CATEGORY_KEY[g.covers] ? t(CATEGORY_KEY[g.covers]) : g.covers}</strong> · {g.agentId ?? t("grants.defaultAgent")} · {titleOf(g.conversationId)}
              {g.lastUsedMs !== undefined && t("grants.used", { ago: agoLabel(g.lastUsedMs, Date.now()) })}{" "}
              <button type="button" className="link-button" onClick={() => void revoke(g)}>
                {t("grants.remove")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
