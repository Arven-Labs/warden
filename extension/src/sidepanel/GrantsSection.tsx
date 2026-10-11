import { useCallback, useEffect, useState } from "react";
import type { Grant } from "../protocol/messages";
import type { GrantsResponse } from "../background/popup_protocol";
import { agoLabel } from "./lib/agentTasks";

const CATEGORY_LABEL: Record<string, string> = {
  delete_data: "Apagar dados",
  spend_money: "Gastar dinheiro",
  critical_infra: "Infraestrutura crítica",
  external_message: "Mensagem externa",
  publish_code: "Publicar código",
  important_config: "Configuração importante",
  elevated_agent: "Agente com permissão elevada",
};

/** P122 — o que a pessoa disse "sempre" nesta conexão: uma categoria de risco ou uma ferramenta, por conversa e agente. Guardado em arquivo,
 * vence depois dos dias que o hub tem configurados sem uso; senão vale até a conexão fechar. Tirar um faz o agente voltar a perguntar; isso
 * só o deixa mais cauteloso, então não pede a chave de pareamento. */
export default function GrantsSection() {
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [ttlDays, setTtlDays] = useState<number | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  const apply = useCallback((res: GrantsResponse) => {
    if (res.ok) {
      setGrants(res.grants);
      setTtlDays(res.ttlDays);
      setError(null);
    } else {
      setError(res.error ?? "falha ao listar as permissões");
    }
  }, []);

  useEffect(() => {
    void chrome.runtime.sendMessage({ type: "listGrants" }).then(apply);
  }, [apply]);

  return (
    <section className="grants-section">
      <h3>Permissões dadas com "sempre"</h3>
      <p className="skills-hint">
        Valem por conversa e agente.{" "}
        {ttlDays !== undefined
          ? `Guardadas em arquivo, sobrevivem a um reinício e vencem depois de ${ttlDays} ${ttlDays === 1 ? "dia" : "dias"} sem uso (cada uso renova).`
          : "Acabam quando a conexão fecha."}{" "}
        Tirar uma faz o agente voltar a perguntar.
      </p>
      {error && <p className="error-banner">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="skills-hint">Nenhuma ainda.</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{CATEGORY_LABEL[g.covers] ?? g.covers}</strong> · {g.agentId ?? "agente padrão"} · conversa {g.conversationId.slice(0, 8)}
              {g.lastUsedMs !== undefined && ` · usado ${agoLabel(g.lastUsedMs, Date.now())}`}{" "}
              <button type="button" className="link-button" onClick={() => void chrome.runtime.sendMessage({ type: "revokeGrant", grant: g }).then(apply)}>
                Tirar
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
