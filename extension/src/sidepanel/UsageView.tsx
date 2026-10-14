import { useCallback, useEffect, useState } from "react";
import type { SpendReport, UsageByKey } from "../protocol/messages";
import type { UsageResponse } from "../background/popup_protocol";
import { comboSpendRows, formatTokens } from "./lib/agentTasks";
import { breakdownRows } from "./lib/usageBreakdown";

const REFRESH_MS = 10000;

function Breakdown({ title, entries, emptyLabel }: { title: string; entries: UsageByKey[] | undefined; emptyLabel: string }) {
  const rows = breakdownRows(entries, emptyLabel);
  if (rows.length === 0) return null;
  return (
    <section className="usage-block">
      <h3 className="activity-day-title">{title}</h3>
      <ul className="agent-work-list">
        {rows.map((row) => (
          <li key={row.key} className="usage-row">
            <div className="usage-row-head">
              <strong>{row.label}</strong>
              <span className="skills-hint">
                {formatTokens(row.tokens)} tokens · {row.calls} {row.calls === 1 ? "chamada" : "chamadas"}
              </span>
            </div>
            <div className="agent-work-bar" role="presentation">
              <div className="agent-work-bar-fill" style={{ width: `${row.share * 100}%` }} />
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** P126 — os tokens do hub inteiro, por agente, por provedor e por combo (o que o turno usou pelo qual o agente começou, quem respondeu e o
 * combo por onde passou). Atualiza enquanto a aba está aberta. Um hub antigo não manda as listas: a tela diz isso em vez de ficar vazia. */
export default function UsageView() {
  const [usage, setUsage] = useState<SpendReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    chrome.runtime.sendMessage({ type: "getUsage" }).then((res: UsageResponse) => {
      if (res.ok) {
        setUsage(res.usage);
        setError(null);
      } else {
        setError(res.error ?? "falha ao ler o uso");
      }
    });
  }, []);

  useEffect(() => {
    load();
    const timer = window.setInterval(load, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [load]);

  const comboSpend = usage === null ? [] : comboSpendRows(usage);
  const told = usage !== null && (usage.byAgent !== undefined || usage.byProvider !== undefined || usage.byCombo !== undefined);
  return (
    <div className="agents-pane">
      <p className="skills-hint">
        Tokens de todas as conversas deste hub, por agente (o que o turno usou, pelo agente com que começou), por provedor (quem respondeu) e por combo (só
        o que passou por um).
      </p>
      {error && <p className="error-banner">{error}</p>}
      {usage === null && !error && <p className="skills-hint">Carregando…</p>}
      {usage !== null && (
        <>
          <p className="usage-totals">
            <strong>{formatTokens(usage.total?.totalTokens ?? 0)}</strong> tokens · <strong>{usage.messageCount ?? 0}</strong> chamadas ao modelo
          </p>
          {!told && <p className="skills-hint">Este hub ainda não informa o uso por agente, provedor e combo.</p>}
          <Breakdown title="Por agente" entries={usage.byAgent} emptyLabel="Sem agente" />
          <Breakdown title="Por provedor" entries={usage.byProvider} emptyLabel="Provedor padrão" />
          <Breakdown title="Por combo" entries={usage.byCombo} emptyLabel="Sem combo" />
          {comboSpend.length > 0 && (
            <section className="usage-block">
              <h3 className="activity-day-title">Gasto por combo</h3>
              <ul className="agent-work-list">
                {comboSpend.map((row) => (
                  <li key={row.combo} className="usage-row">
                    <div className="usage-row-head">
                      <strong>{row.combo}</strong>
                      <span className="skills-hint">{row.label}</span>
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}
