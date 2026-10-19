import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { LimitStatus, SpendBucket, UsageByKey, UsageReport } from "../hub/messages";
import { breakdownRows } from "../hub/usageBreakdown";
import { intlTag, t } from "../i18n";

// Usage across the whole hub (P78): tokens from every device's conversations (all time), and the
// spending limits and recent dollars from the P4 ledger, which every channel on the hub's machine
// shares. A limit that ran out can be extended from here, like the desktop's pause dialog.

// Read at each use, so a switch of language shows at once.
const compact = { format: (n: number) => new Intl.NumberFormat(intlTag(), { notation: "compact", maximumFractionDigits: 1 }).format(n) };
const full = { format: (n: number) => new Intl.NumberFormat(intlTag()).format(n) };

function usd(value: number): string {
  const prefix = intlTag() === "pt-BR" ? "US$ " : "$";
  return `${prefix}${value.toLocaleString(intlTag(), { minimumFractionDigits: 2, maximumFractionDigits: value < 1 ? 4 : 2 })}`;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** `2026-09-25` → `25/09`. */
function shortDate(date: string): string {
  const [, month, day] = date.split("-");
  return `${day}/${month}`;
}

/** `LimitStatus.scope` as the hub writes it (`global`, `agent x`, `channel x`, `user channel:id`). */
function scopeLabel(scope: string): string {
  const [kind, ...rest] = scope.split(" ");
  const target = rest.join(" ");
  switch (kind) {
    case "global":
      return t("usage.scope.global");
    case "agent":
      return t("usage.scope.agent", { target });
    case "channel":
      return t("usage.scope.channel", { target });
    case "user":
      return t("usage.scope.user", { target });
    case "person":
      return t("usage.scope.person");
    default:
      return scope;
  }
}

function windowLabel(hours: number): string {
  return hours % 24 === 0 ? (hours === 24 ? "24 h" : t("usage.days", { count: hours / 24 })) : `${hours} h`;
}

function Tile({ label, value, title }: { label: string; value: string; title?: string }) {
  return (
    <div className="usage-tile" title={title}>
      <span className="usage-tile-value">{value}</span>
      <span className="usage-tile-label">{label}</span>
    </div>
  );
}

/** One day of a daily chart: whatever it measures (tokens, dollars) is `value`. */
interface DayRow {
  date: string;
  calls: number;
  value: number;
}

/** One value per day (tokens, or dollars in P10): one series, so no legend — the heading names it. Each column
 * is its own hover and focus target; the table below carries the exact numbers. `axis` writes the top of the
 * scale, `describe` a day's value in full, `column` heads the table's value column. */
function DailyChart({ rows: daily, axis, describe, column }: { rows: DayRow[]; axis: (value: number) => string; describe: (value: number) => string; column: string }) {
  const max = Math.max(...daily.map((d) => d.value), 0);
  return (
    <figure className="usage-daily">
      <div className="usage-daily-plot">
        <span className="usage-daily-axis">{max > 0 ? axis(max) : "0"}</span>
        <div className="usage-daily-columns" role="list">
          {daily.map((d) => (
            <div
              key={d.date}
              role="listitem"
              tabIndex={0}
              className="usage-daily-slot"
              aria-label={t("usage.dayAria", { date: shortDate(d.date), value: describe(d.value), calls: d.calls })}
            >
              {d.value > 0 && <div className="usage-daily-bar" style={{ height: `${Math.max((d.value / max) * 100, 2)}%` }} />}
              <span className="usage-daily-tip" role="tooltip">
                <strong>{shortDate(d.date)}</strong>
                {describe(d.value)} · {d.calls} {t(d.calls === 1 ? "usage.callOne" : "usage.callMany")}
              </span>
            </div>
          ))}
        </div>
      </div>
      <div className="usage-daily-dates">
        <span>{daily.length > 0 && shortDate(daily[0].date)}</span>
        <span>{t("usage.today")}</span>
      </div>
      <details className="usage-table-toggle">
        <summary>{t("usage.viewTable")}</summary>
        <table className="usage-table">
          <thead>
            <tr>
              <th>{t("usage.col.day")}</th>
              <th>{t("usage.col.calls")}</th>
              <th>{column}</th>
            </tr>
          </thead>
          <tbody>
            {daily
              .filter((d) => d.calls > 0)
              .reverse()
              .map((d) => (
                <tr key={d.date}>
                  <td>{shortDate(d.date)}</td>
                  <td>{full.format(d.calls)}</td>
                  <td>{describe(d.value)}</td>
                </tr>
              ))}
          </tbody>
        </table>
      </details>
    </figure>
  );
}

function LimitCard({ limit, busy, onExtend }: { limit: LimitStatus; busy: boolean; onExtend: (() => void) | null }) {
  const state = limit.exceeded ? "exceeded" : limit.warn ? "warn" : "ok";
  const parts: string[] = [];
  if (limit.maxTokens !== null) parts.push(t("usage.tokensOf", { used: full.format(limit.usedTokens), max: full.format(limit.maxTokens) }));
  if (limit.maxCostUsd !== null) parts.push(`${usd(limit.usedCostUsd)} / ${usd(limit.maxCostUsd)}`);
  const extension = [
    limit.extendTokens > 0 ? `+${compact.format(limit.extendTokens)} tokens` : null,
    limit.extendCostUsd > 0 ? `+${usd(limit.extendCostUsd)}` : null,
  ]
    .filter(Boolean)
    .join(" / ");

  return (
    <li className={`usage-limit usage-limit--${state}`}>
      <div className="usage-limit-header">
        <span className="usage-limit-name">{limit.id}</span>
        <span className="usage-limit-scope">
          {scopeLabel(limit.scope)} · {t("usage.lastWindow", { window: windowLabel(limit.windowHours) })}
        </span>
      </div>
      <div
        className="usage-meter"
        role="meter"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(Math.min(limit.fraction, 1) * 100)}
        aria-label={t("usage.limit", { id: limit.id })}
      >
        <div className="usage-meter-fill" style={{ width: `${Math.min(limit.fraction, 1) * 100}%` }} />
      </div>
      <div className="usage-limit-detail">
        <span>
          {state === "exceeded" ? t("usage.exhausted") : state === "warn" ? t("usage.nearLimit") : t("usage.roomLeft")} · {Math.round(limit.fraction * 100)}% ·{" "}
          {parts.join(" · ")}
        </span>
        {limit.freesUpInMinutes !== null && limit.fraction > 0 && <span className="skills-hint">{t("usage.freesUp", { minutes: limit.freesUpInMinutes })}</span>}
        {limit.unpricedCalls > 0 && limit.maxCostUsd !== null && (
          <span className="skills-hint">{t(limit.unpricedCalls === 1 ? "usage.unpricedOne" : "usage.unpricedMany", { count: limit.unpricedCalls })}</span>
        )}
      </div>
      {onExtend && state !== "ok" && extension && (
        <button type="button" className="link-button usage-limit-extend" disabled={busy} onClick={onExtend}>
          {busy ? t("usage.releasing") : t("usage.release", { extension })}
        </button>
      )}
    </li>
  );
}

/** Tokens by agent, provider or combo (P126), drawn like the devices' bars. Nothing to show hides it (an older hub sends none). */
function TokenBreakdown({ title, entries, emptyLabel }: { title: string; entries: UsageByKey[] | undefined; emptyLabel: string }) {
  const rows = breakdownRows(entries, emptyLabel);
  if (rows.length === 0) return null;
  return (
    <section className="usage-section">
      <h2 className="usage-heading">{title}</h2>
      <ul className="usage-bars">
        {rows.map((r) => (
          <li key={r.key} className="usage-bar-row">
            <span className="usage-bar-label" title={r.label}>
              {r.label}
            </span>
            <div className="usage-bar-track">
              <div className="usage-bar-fill" style={{ width: `${r.share * 100}%` }} />
            </div>
            <span className="usage-bar-value" title={t("usage.tokensIn", { tokens: full.format(r.tokens), calls: full.format(r.calls) })}>
              {compact.format(r.tokens)}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** `emptyLabel` is what a bucket with an empty key is called: a call with no provider, agent or person on record. */
function SpendTable({ title, buckets, keyLabel, emptyLabel }: { title: string; buckets: SpendBucket[]; keyLabel: string; emptyLabel?: string }) {
  if (buckets.length === 0) return null;
  return (
    <table className="usage-table">
      <caption>{title}</caption>
      <thead>
        <tr>
          <th>{keyLabel}</th>
          <th>{t("usage.col.calls")}</th>
          <th>{t("usage.col.tokens")}</th>
          <th>{t("usage.col.usd")}</th>
        </tr>
      </thead>
      <tbody>
        {buckets.map((b) => (
          <tr key={b.key}>
            <td className="usage-table-key">{b.key === "" && emptyLabel ? emptyLabel : b.key}</td>
            <td>{full.format(b.calls)}</td>
            <td>{full.format(b.tokens)}</td>
            <td>
              {b.calls > b.unpricedCalls ? usd(b.costUsd) : "—"}
              {b.unpricedCalls > 0 && b.calls > b.unpricedCalls && <span className="skills-hint"> {t("usage.unpricedShort", { count: b.unpricedCalls })}</span>}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** `readOnly` is a workspace member's view of their own use (P99): no way to allow more, and nothing that is about other people or devices. */
export default function UsageView({ conn, readOnly = false }: { conn: ServerConnection | null; readOnly?: boolean }) {
  const [report, setReport] = useState<UsageReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [extending, setExtending] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.requestUsage().then(
      (r) => {
        setReport(r);
        setError(null);
      },
      (err) => setError(t("usage.loadError", { error: message(err) })),
    );
  }, [conn]);

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(refresh, [refresh]);

  async function extend(limitId: string) {
    if (!conn) return;
    setExtending(limitId);
    try {
      await conn.extendLimit(limitId);
      refresh();
    } catch (err) {
      setError(t("usage.releaseError", { error: message(err) }));
    } finally {
      setExtending(null);
    }
  }

  if (report === null) {
    return <div className="usage-view">{error ? <p className="error-banner">{error}</p> : <p className="skills-hint">{t("usage.loading")}</p>}</div>;
  }

  const maxDevice = Math.max(...report.byDevice.map((d) => d.usage.totalTokens), 1);
  const totalCost = report.recent?.byModel.reduce((sum, b) => sum + b.costUsd, 0) ?? 0;

  return (
    <div className="usage-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("usage.intro")}</span>
        <button type="button" className="link-button" onClick={refresh} disabled={!conn}>
          {t("usage.refresh")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}

      <div className="usage-tiles">
        <Tile label={t("usage.tile.tokens")} value={compact.format(report.total.totalTokens)} title={`${full.format(report.total.totalTokens)} tokens`} />
        <Tile label={t("usage.tile.calls")} value={compact.format(report.messageCount)} />
        <Tile label={t("usage.tile.conversations")} value={compact.format(report.conversationCount)} />
        {report.recent && (
          <Tile label={t("usage.tile.spend", { window: windowLabel(report.recent.windowHours) })} value={totalCost > 0 ? usd(totalCost) : "—"} />
        )}
      </div>

      <section className="usage-section">
        <h2 className="usage-heading">{t("usage.tokensPerDay", { count: report.daily.length })}</h2>
        <DailyChart
          rows={report.daily.map((d) => ({ date: d.date, calls: d.calls, value: d.tokens }))}
          axis={(n) => compact.format(n)}
          describe={(n) => `${full.format(n)} tokens`}
          column={t("usage.col.tokens")}
        />
      </section>

      {report.recent && report.dailyCost && report.dailyCost.length > 0 && (
        <section className="usage-section">
          <h2 className="usage-heading">{t("usage.spendPerDay", { window: windowLabel(report.recent.windowHours) })}</h2>
          <p className="skills-hint">{t("usage.spendPerDayHint", { window: windowLabel(report.recent.windowHours) })}</p>
          <DailyChart
            rows={report.dailyCost.map((d) => ({ date: d.date, calls: d.calls, value: d.costUsd }))}
            axis={usd}
            describe={usd}
            column={t("usage.col.usd")}
          />
        </section>
      )}

      <section className="usage-section">
        <h2 className="usage-heading">{t("usage.limits")}</h2>
        {report.ledgerError && <p className="error-banner">{t("usage.ledgerError", { error: report.ledgerError })}</p>}
        {!report.limitsEnabled ? (
          <p className="skills-hint">{t("usage.limitsOff")}</p>
        ) : report.limits.length === 0 ? (
          <p className="skills-hint">{t("usage.noLimits")}</p>
        ) : (
          <ul className="usage-limits">
            {report.limits.map((limit) => (
              <LimitCard key={limit.id} limit={limit} busy={extending === limit.id || !conn} onExtend={readOnly ? null : () => void extend(limit.id)} />
            ))}
          </ul>
        )}
        {readOnly && <p className="skills-hint">{t("usage.onlyYours")}</p>}
      </section>

      {!readOnly && (
        <section className="usage-section">
          <h2 className="usage-heading">{t("usage.byDevice")}</h2>
          {report.byDevice.length === 0 ? (
            <p className="skills-hint">{t("usage.noConversations")}</p>
          ) : (
            <ul className="usage-bars">
              {report.byDevice.map((d) => (
                <li key={d.deviceId} className="usage-bar-row">
                  <span className="usage-bar-label" title={d.deviceId}>
                    {d.name ?? d.deviceId}
                  </span>
                  <div className="usage-bar-track">
                    <div className="usage-bar-fill" style={{ width: `${(d.usage.totalTokens / maxDevice) * 100}%` }} />
                  </div>
                  <span className="usage-bar-value" title={t("usage.deviceTitle", { tokens: full.format(d.usage.totalTokens), count: d.conversationCount })}>
                    {compact.format(d.usage.totalTokens)}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      <TokenBreakdown title={t("usage.byAgentTokens")} entries={report.byAgent} emptyLabel={t("usage.noAgent")} />
      <TokenBreakdown title={t("usage.byProviderTokens")} entries={report.byProvider} emptyLabel={t("usage.defaultProvider")} />
      <TokenBreakdown title={t("usage.byComboTokens")} entries={report.byCombo} emptyLabel={t("usage.noCombo")} />

      {report.recent && (report.recent.byModel.length > 0 || report.recent.byChannel.length > 0) && (
        <section className="usage-section">
          <h2 className="usage-heading">{t("usage.recentSpend", { window: windowLabel(report.recent.windowHours) })}</h2>
          <p className="skills-hint">
            {t("usage.recentHint")} {readOnly ? t("usage.recentYours") : t("usage.recentOwner")}
          </p>
          <SpendTable title={t("usage.byModel")} buckets={report.recent.byModel} keyLabel={t("usage.model")} />
          <SpendTable title={t("usage.byChannel")} buckets={report.recent.byChannel} keyLabel={t("usage.channel")} />
          <SpendTable title={t("usage.byProvider")} buckets={report.recent.byProvider ?? []} keyLabel={t("usage.provider")} emptyLabel={t("usage.noProviderRecorded")} />
          <SpendTable title={t("usage.byAgent")} buckets={report.recent.byAgent ?? []} keyLabel={t("usage.agent")} emptyLabel={t("usage.noAgentLower")} />
          {!readOnly && <SpendTable title={t("usage.byPerson")} buckets={report.recent.byPerson ?? []} keyLabel={t("usage.person")} emptyLabel={t("usage.theOwner")} />}
          <SpendTable title={t("usage.byCombo")} buckets={report.recent.byCombo ?? []} keyLabel={t("usage.combo")} />
          <p className="skills-hint">{t("usage.footnote")}</p>
        </section>
      )}
    </div>
  );
}
