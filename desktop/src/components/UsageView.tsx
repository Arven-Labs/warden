import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { intlTag, t } from "../i18n";
import type { Credit, RecentSpend, SpendBucket, SpendStatus, UsageByKey, UsageSummary } from "../types";
import { hubUsage } from "../lib/hub";
import { creditUsd, hasHolds, isOutOfCredit } from "../lib/credit";

/** `1,284` up to 999, `12.9K`/`1.2M` beyond — the stat-tile contract's "auto-compact" value
 * format, so a heavy user's token count never wraps a tile onto two lines. */
function formatCompact(n: number): string {
  return new Intl.NumberFormat(intlTag(), { notation: "compact", maximumFractionDigits: 1 }).format(n);
}

/** A number is compacted; a string (the credit, in dollars) is shown as given. `title` is the hover hint. */
function StatTile({ label, value, title }: { label: string; value: number | string; title?: string }) {
  return (
    <div className="usage-stat-tile" title={title}>
      <span className="usage-stat-value">{typeof value === "number" ? formatCompact(value) : value}</span>
      <span className="usage-stat-label">{label}</span>
    </div>
  );
}

/** A magnitude-by-category breakdown (by agent, by provider) — one accent-colored bar per row,
 * `key` already carries the display name (`AgentEntry`/`ProviderEntry.id` doubles as its own
 * name, see `types.ts`), so no separate name lookup against Settings is needed. One series (the
 * same metric, total tokens, repeated per category) needs no legend — identity comes from the
 * row label, not the bar's color. Value sits at the bar's tip, the direct-label spec for bars. */
function UsageBreakdown({ title, entries, emptyKeyLabel }: { title: string; entries: UsageByKey[]; emptyKeyLabel: string }) {
  if (entries.length === 0) return null;
  const maxTokens = Math.max(...entries.map((entry) => entry.usage.totalTokens), 1);

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{title}</h3>
      </div>
      <div className="usage-bar-list">
        {entries.map((entry) => (
          <div className="usage-bar-row" key={entry.key ?? "none"}>
            <span className="usage-bar-label">{entry.key ?? emptyKeyLabel}</span>
            <div className="usage-bar-track">
              <div className="usage-bar-fill" style={{ width: `${(entry.usage.totalTokens / maxTokens) * 100}%` }} />
            </div>
            <span className="usage-bar-value">{formatCompact(entry.usage.totalTokens)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

/** Where the numbers come from: this computer's guard and conversations, or the hub in use. */
interface UsageSource {
  summary: () => Promise<UsageSummary>;
  spend: () => Promise<SpendStatus>;
  extend: (limitId: string) => Promise<void>;
}

const localUsage: UsageSource = {
  summary: () => invoke<UsageSummary>("usage_summary"),
  spend: () => invoke<SpendStatus>("spend_status"),
  extend: (limitId) => invoke("extend_spend_limit", { limitId }),
};

function usd(value: number): string {
  return `$${value.toFixed(value < 1 ? 4 : 2)}`;
}

/** The hosted credit: the balance as a tile (with the available amount as its hint while calls hold part of it) and a warning when
 * nothing is left. Shown on its own, because a new hosted account has credit before it has a single message. */
function CreditCard({ credit }: { credit: Credit }) {
  return (
    <>
      {isOutOfCredit(credit) && <p className="usage-error">{t("usage.creditEmpty")}</p>}
      <div className="usage-stat-grid">
        <StatTile
          label={t("usage.credit")}
          value={usd(creditUsd(credit.balanceMicroUsd))}
          title={hasHolds(credit) ? t("usage.creditAvailable", { amount: usd(creditUsd(credit.availableMicroUsd)) }) : undefined}
        />
      </div>
    </>
  );
}

/** One split of the ledger's spending, with the dollars. A call to a model with no price has no dollar figure,
 * so a row made only of those says "—" instead of a false zero. `emptyLabel` names the bucket with an empty key. */
function SpendTable({ title, buckets, keyLabel, emptyLabel }: { title: string; buckets: SpendBucket[]; keyLabel: string; emptyLabel?: string }) {
  if (buckets.length === 0) return null;
  return (
    <table className="usage-table">
      <caption>{title}</caption>
      <thead>
        <tr>
          <th>{keyLabel}</th>
          <th>{t("usage.calls")}</th>
          <th>{t("usage.tokens")}</th>
          <th>USD</th>
        </tr>
      </thead>
      <tbody>
        {buckets.map((b) => (
          <tr key={b.key}>
            <td>{b.key === "" && emptyLabel ? emptyLabel : b.key}</td>
            <td>{b.calls.toLocaleString(intlTag())}</td>
            <td>{b.tokens.toLocaleString(intlTag())}</td>
            <td>
              {b.calls > b.unpricedCalls ? usd(b.costUsd) : "—"}
              {b.unpricedCalls > 0 && b.calls > b.unpricedCalls && <span className="settings-hint">{t("usage.unpriced", { count: b.unpricedCalls })}</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** What the spending ledger holds, split five ways, in dollars (P10). Reads the same status as the limits above it. */
function RecentSpending({ source }: { source: UsageSource }) {
  const [recent, setRecent] = useState<RecentSpend | null>(null);

  useEffect(() => {
    source
      .spend()
      .then((status) => setRecent(status.recent))
      .catch(() => setRecent(null));
  }, [source]);

  if (recent === null || recent.byModel.length === 0) return null;
  const hours = recent.windowHours;
  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">
          {hours % 24 === 0
            ? hours === 24
              ? t("usage.recentDay")
              : t("usage.recentDays", { count: hours / 24 })
            : t("usage.recentHours", { count: hours })}
        </h3>
      </div>
      <p className="settings-hint">{t("usage.recentHint")}</p>
      <SpendTable title={t("usage.byModel")} buckets={recent.byModel} keyLabel={t("usage.model")} />
      <SpendTable title={t("usage.byChannel")} buckets={recent.byChannel} keyLabel={t("usage.channel")} />
      <SpendTable title={t("usage.byProvider")} buckets={recent.byProvider} keyLabel={t("usage.provider")} emptyLabel={t("usage.noProviderRecorded")} />
      <SpendTable title={t("usage.byAgent")} buckets={recent.byAgent} keyLabel={t("usage.agent")} emptyLabel={t("usage.noAgentLower")} />
      <SpendTable title={t("usage.byPerson")} buckets={recent.byPerson} keyLabel={t("usage.person")} emptyLabel={t("usage.owner")} />
      <SpendTable title={t("usage.byCombo")} buckets={recent.byCombo ?? []} keyLabel={t("usage.combo")} />
      <p className="settings-hint">{t("usage.recentNote")}</p>
    </section>
  );
}

/** Where each spending limit (P4) stands, with "Allow more" on one that's running out — the same
 * grant the pause dialog makes mid-turn. The meter's fill carries severity (accent → warning →
 * danger), always next to an icon and a label, never color alone. */
function SpendingLimits({ source }: { source: UsageSource }) {
  const [status, setStatus] = useState<SpendStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [extending, setExtending] = useState<string | null>(null);

  function refresh() {
    source
      .spend()
      .then(setStatus)
      .catch((err) => setError(String(err)));
  }

  useEffect(refresh, [source]);

  async function extend(limitId: string) {
    setExtending(limitId);
    setError(null);
    try {
      await source.extend(limitId);
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setExtending(null);
    }
  }

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("limits.title")}</h3>
      </div>
      {error && <p className="settings-error-banner">{error}</p>}
      {status?.ledgerError && <p className="settings-error-banner">{t("limits.ledgerError", { error: status.ledgerError })}</p>}
      {status === null ? (
        !error && <p className="settings-hint">{t("common.loading")}</p>
      ) : !status.limitsEnabled ? (
        <p className="settings-hint">{t("limits.off")}</p>
      ) : status.limits.length === 0 ? (
        <p className="settings-hint">{t("limits.none")}</p>
      ) : (
        <div className="spend-limit-list">
          {status.limits.map((limit) => {
            const state = limit.exceeded ? "exceeded" : limit.warn ? "warn" : "ok";
            const parts = [
              limit.maxTokens !== null ? t("limits.usedTokens", { used: limit.usedTokens.toLocaleString(intlTag()), max: limit.maxTokens.toLocaleString(intlTag()) }) : null,
              limit.maxCostUsd !== null ? t("limits.usedCost", { used: usd(limit.usedCostUsd), max: usd(limit.maxCostUsd) }) : null,
            ].filter(Boolean);
            const extension = [
              limit.extendTokens > 0 ? t("limits.extTokens", { count: formatCompact(limit.extendTokens) }) : null,
              limit.extendCostUsd > 0 ? `+${usd(limit.extendCostUsd)}` : null,
            ]
              .filter(Boolean)
              .join(" / ");
            return (
              <div key={limit.id} className={`spend-limit spend-limit--${state}`}>
                <div className="spend-limit-header">
                  <span className="spend-limit-name">{limit.id}</span>
                  <span className="settings-hint">{t("limits.scopeWindow", { scope: limit.scope, hours: limit.windowHours })}</span>
                </div>
                <div
                  className="spend-meter"
                  role="meter"
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuenow={Math.round(Math.min(limit.fraction, 1) * 100)}
                  aria-label={t("limits.label", { id: limit.id })}
                >
                  <div className="spend-meter-fill" style={{ width: `${Math.min(limit.fraction, 1) * 100}%` }} />
                </div>
                <span className="spend-limit-detail">
                  {t("limits.detail", {
                    state: state === "exceeded" ? t("limits.exhausted") : state === "warn" ? t("limits.low") : t("limits.ok"),
                    percent: Math.round(limit.fraction * 100),
                    parts: parts.join(" · "),
                  })}
                </span>
                {limit.freesUpInMinutes !== null && limit.fraction > 0 && (
                  <span className="settings-hint">{t("limits.freesUp", { minutes: limit.freesUpInMinutes })}</span>
                )}
                {limit.unpricedCalls > 0 && limit.maxCostUsd !== null && (
                  <span className="settings-hint">{t("limits.unpricedCalls", { count: limit.unpricedCalls })}</span>
                )}
                {state !== "ok" && extension && (
                  <button type="button" className="settings-browse-btn spend-limit-extend" disabled={extending === limit.id} onClick={() => void extend(limit.id)}>
                    {extending === limit.id ? t("limits.allowing") : t("limits.allow", { extension })}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

/** `remote`: the numbers are the hub's (P102). The caller remounts the screen (`key`) when the machine changes. */
function UsageView({ remote = false }: { remote?: boolean }) {
  const source = useMemo(() => (remote ? hubUsage() : localUsage), [remote]);
  const [summary, setSummary] = useState<UsageSummary | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Fetched fresh every time this view mounts (the sidebar remounts it on each visit, same as
  // `SettingsView`) rather than cached in `App` — a dashboard showing yesterday's numbers right
  // after sending a message would be worse than the extra disk read.
  useEffect(() => {
    source
      .summary()
      .then(setSummary)
      .catch((err) => setError(String(err)));
  }, [source]);

  if (error) {
    return (
      <div className="settings-view">
        <h2 className="settings-title">{t("usage.title")}</h2>
        <p className="usage-error">{error}</p>
      </div>
    );
  }

  if (!summary) {
    return (
      <div className="settings-view">
        <p>{t("usage.loading")}</p>
      </div>
    );
  }

  if (summary.messageCount === 0) {
    return (
      <div className="settings-view">
        <h2 className="settings-title">{t("usage.title")}</h2>
        <p className="settings-hint">{t("usage.none")}</p>
        {summary.credit && <CreditCard credit={summary.credit} />}
        <SpendingLimits source={source} />
        <RecentSpending source={source} />
      </div>
    );
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("usage.title")}</h2>
      <p className="settings-hint">{remote ? t("usage.hintHub") : t("usage.hintLocal")}</p>

      {summary.credit && <CreditCard credit={summary.credit} />}
      <SpendingLimits source={source} />
      <RecentSpending source={source} />

      <div className="usage-stat-grid">
        <StatTile label={t("usage.totalTokens")} value={summary.total.totalTokens} />
        <StatTile label={t("usage.promptTokens")} value={summary.total.promptTokens} />
        <StatTile label={t("usage.completionTokens")} value={summary.total.completionTokens} />
        <StatTile label={t("usage.modelCalls")} value={summary.messageCount} />
        <StatTile label={t("usage.conversations")} value={summary.conversationCount} />
      </div>

      <UsageBreakdown title={t("usage.byAgent")} entries={summary.byAgent} emptyKeyLabel={t("usage.noAgent")} />
      <UsageBreakdown title={t("usage.byProvider")} entries={summary.byProvider} emptyKeyLabel={t("usage.defaultProvider")} />
      <UsageBreakdown title={t("usage.byCombo")} entries={summary.byCombo ?? []} emptyKeyLabel={t("usage.noCombo")} />
    </div>
  );
}

export default UsageView;
