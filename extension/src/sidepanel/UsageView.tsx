import { useCallback, useEffect, useState } from "react";
import type { SpendReport, UsageByKey } from "../protocol/messages";
import type { UsageResponse } from "../background/popup_protocol";
import { comboSpendRows, formatTokens } from "./lib/agentTasks";
import { breakdownRows } from "./lib/usageBreakdown";
import { t } from "../i18n/index.ts";
import { rich } from "../i18n/react.tsx";

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
                {t("usageView.callsLine", { tokens: formatTokens(row.tokens), count: row.calls, unit: t(row.calls === 1 ? "usageView.callOne" : "usageView.callMany") })}
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

/** P126 — the whole hub's tokens, by agent, by provider and by combo (what the turn used by the agent it started with, who answered and the
 * combo it went through). Updates while the tab is open. An older hub does not send the lists: the screen says so instead of staying empty. */
export default function UsageView() {
  const [usage, setUsage] = useState<SpendReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    chrome.runtime.sendMessage({ type: "getUsage" }).then((res: UsageResponse) => {
      if (res.ok) {
        setUsage(res.usage);
        setError(null);
      } else {
        setError(res.error ?? t("usageView.readFailed"));
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
      <p className="skills-hint">{t("usageView.intro")}</p>
      {error && <p className="error-banner">{error}</p>}
      {usage === null && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {usage !== null && (
        <>
          <p className="usage-totals">
            {rich("usageView.totals", {
              tokens: <strong>{formatTokens(usage.total?.totalTokens ?? 0)}</strong>,
              count: <strong>{usage.messageCount ?? 0}</strong>,
            })}
          </p>
          {!told && <p className="skills-hint">{t("usageView.notTold")}</p>}
          <Breakdown title={t("usageView.byAgent")} entries={usage.byAgent} emptyLabel={t("usageView.noAgent")} />
          <Breakdown title={t("usageView.byProvider")} entries={usage.byProvider} emptyLabel={t("usageView.defaultProvider")} />
          <Breakdown title={t("usageView.byCombo")} entries={usage.byCombo} emptyLabel={t("usageView.noCombo")} />
          {comboSpend.length > 0 && (
            <section className="usage-block">
              <h3 className="activity-day-title">{t("usageView.spendByCombo")}</h3>
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
