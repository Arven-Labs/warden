import { useCallback, useEffect, useState } from "react";
import type { Grant } from "../protocol/messages";
import type { GrantsResponse } from "../background/popup_protocol";
import { t, type Key } from "../i18n/index.ts";
import { agoLabel } from "./lib/agentTasks";

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
      setError(res.error ?? t("grants.listFailed"));
    }
  }, []);

  useEffect(() => {
    void chrome.runtime.sendMessage({ type: "listGrants" }).then(apply);
  }, [apply]);

  return (
    <section className="grants-section">
      <h3>{t("grants.title")}</h3>
      <p className="skills-hint">
        {t("grants.scope")}{" "}
        {ttlDays !== undefined ? t("grants.kept", { days: ttlDays, unit: t(ttlDays === 1 ? "grants.day" : "grants.days") }) : t("grants.session")}{" "}
        {t("grants.takeBack")}
      </p>
      {error && <p className="error-banner">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="skills-hint">{t("grants.none")}</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{CATEGORY_KEY[g.covers] ? t(CATEGORY_KEY[g.covers]) : g.covers}</strong> · {g.agentId ?? t("grants.defaultAgent")} ·{" "}
              {t("grants.conversation", { id: g.conversationId.slice(0, 8) })}
              {g.lastUsedMs !== undefined && t("grants.used", { ago: agoLabel(g.lastUsedMs, Date.now()) })}{" "}
              <button type="button" className="link-button" onClick={() => void chrome.runtime.sendMessage({ type: "revokeGrant", grant: g }).then(apply)}>
                {t("grants.remove")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
