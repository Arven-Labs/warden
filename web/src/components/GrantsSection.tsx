import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { ConversationSummary, Grant } from "../hub/messages";
import { agoLabel } from "../hub/agentTasks";

const CATEGORY_LABEL: Record<string, string> = {
  delete_data: "Apagar dados",
  spend_money: "Gastar dinheiro",
  critical_infra: "Infraestrutura crítica",
  external_message: "Mensagem externa",
  publish_code: "Publicar código",
  important_config: "Configuração importante",
  elevated_agent: "Agente com permissão elevada",
};

/** P122 — o que a pessoa disse "sempre" nesta conexão: uma categoria de risco ou uma ferramenta, por conversa e agente. Vale só até a
 * conexão fechar; aqui dá para ver e tirar um, e o agente volta a perguntar. */
export default function GrantsSection({ conn, conversations }: { conn: ServerConnection | null; conversations: ConversationSummary[] }) {
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      setGrants(await conn.listGrants());
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
      setGrants(await conn.revokeGrant(grant));
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  const titleOf = (id: string) => conversations.find((c) => c.id === id)?.title || id;

  return (
    <section className="grants-section">
      <h3>Permissões dadas com "sempre"</h3>
      <p className="skills-hint">
        Valem por conversa e agente. Se o hub guarda em arquivo (o padrão), sobrevivem a um reinício e vencem depois de 30 dias sem uso
        (cada uso renova); senão, acabam quando a conexão fecha. Tirar uma faz o agente voltar a perguntar.
      </p>
      {error && <p className="error-banner">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="skills-hint">Nenhuma ainda.</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{CATEGORY_LABEL[g.covers] ?? g.covers}</strong> · {g.agentId ?? "agente padrão"} · {titleOf(g.conversationId)}
              {g.lastUsedMs !== undefined && ` · usado ${agoLabel(g.lastUsedMs, Date.now())}`}{" "}
              <button type="button" className="link-button" onClick={() => void revoke(g)}>
                Tirar
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
