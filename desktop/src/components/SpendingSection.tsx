import { useEffect, useState } from "react";
import { intlTag, t } from "../i18n";
import { rich } from "../i18n/rich";
import type { AgentEntry, LimitEntry, LimitScope, PriceEntry } from "../types";

/** The channels a limit can be about — the names `SpendContext` gets from each of them. */
const CHANNELS = ["desktop", "cli", "telegram", "whatsapp", "server"];

const scopeOptions = (): { value: LimitScope; label: string }[] => [
  { value: "global", label: t("spending.scopeGlobal") },
  { value: "agent", label: t("spending.scopeAgent") },
  { value: "channel", label: t("spending.scopeChannel") },
  { value: "user", label: t("spending.scopeUser") },
  { value: "person", label: t("spending.scopePerson") },
];

/** A blank field is `null`; anything that is not a number is `null` too, and the submit check
 * (`validateSpending`) is what tells the person, so a half-typed value never blocks typing. */
function parseNumber(raw: string, integer: boolean): number | null {
  const text = raw.trim();
  if (text === "") return null;
  const n = Number(text);
  if (!Number.isFinite(n)) return null;
  return integer ? Math.round(n) : n;
}

/** A text box for a number. It keeps what was typed (`0.` must stay `0.` while the next digit is
 * on its way) and only hands the parsed number up — resyncing when the value changes from outside,
 * like "Restore safety net". */
function NumberField({
  label,
  value,
  onChange,
  integer = false,
  placeholder,
  ariaLabel,
}: {
  label: string;
  value: number | null;
  onChange: (next: number | null) => void;
  integer?: boolean;
  placeholder?: string;
  ariaLabel?: string;
}) {
  const [draft, setDraft] = useState(value === null ? "" : String(value));
  useEffect(() => {
    setDraft((current) => (parseNumber(current, integer) === value ? current : value === null ? "" : String(value)));
  }, [value, integer]);
  return (
    <label className="settings-field">
      <span className="settings-label">{label}</span>
      <input
        className="settings-input"
        type="text"
        inputMode="decimal"
        placeholder={placeholder}
        aria-label={ariaLabel ?? label}
        value={draft}
        onChange={(e) => {
          const text = e.currentTarget.value;
          setDraft(text);
          onChange(parseNumber(text, integer));
        }}
      />
    </label>
  );
}

/** A fraction (0–1) shown as a percentage — `0.8` reads `80`. Rounded so `0.8 * 100` doesn't show
 * as `80.00000000000001`. */
function PercentField({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  value: number | null;
  onChange: (next: number | null) => void;
  placeholder: string;
}) {
  return (
    <NumberField
      label={label}
      placeholder={placeholder}
      value={value === null ? null : Math.round(value * 1e4) / 100}
      onChange={(percent) => onChange(percent === null ? null : percent / 100)}
    />
  );
}

function nextLimitId(existing: LimitEntry[]): string {
  let n = existing.length + 1;
  while (existing.some((l) => l.id === `limit-${n}`)) n += 1;
  return `limit-${n}`;
}

const formatTokens = (n: number) => n.toLocaleString(intlTag());

/** The limit in one plain sentence, so the numbers don't have to be decoded from the form. */
export function describeLimit(limit: LimitEntry): string {
  const who =
    limit.scope === "global"
      ? t("spending.whoGlobal")
      : limit.scope === "agent"
        ? t("spending.whoAgent", { target: limit.target || "…" })
        : limit.scope === "channel"
          ? t("spending.whoChannel", { target: limit.target || "…" })
          : `${limit.target || "…"}`;
  const ceilings = [
    limit.maxTokens !== null ? t("spending.tokensCap", { count: formatTokens(limit.maxTokens) }) : null,
    limit.maxCostUsd !== null ? `$${limit.maxCostUsd}` : null,
  ].filter((c) => c !== null);
  const window = limit.windowHours === 1 ? t("spending.window1") : t("spending.windowN", { count: limit.windowHours });
  return t("spending.describe", { who: `${who[0].toUpperCase()}${who.slice(1)}`, ceilings: ceilings.join(t("spending.or")) || "…", window });
}

function LimitCard({
  limit,
  agents,
  onChange,
  onDelete,
}: {
  limit: LimitEntry;
  agents: AgentEntry[];
  onChange: (next: LimitEntry) => void;
  onDelete: () => void;
}) {
  const knownAgent = agents.some((a) => a.id === limit.target);
  const knownChannel = CHANNELS.includes(limit.target);
  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          placeholder={t("spending.namePlaceholder")}
          value={limit.id}
          onChange={(e) => onChange({ ...limit, id: e.currentTarget.value })}
        />
        <button
          type="button"
          className="provider-delete-btn"
          onClick={onDelete}
          aria-label={t("spending.deleteLabel", { name: limit.id || t("spending.thisLimit") })}
          title={t("spending.deleteTitle")}
        >
          🗑
        </button>
      </div>

      <div className="spend-grid">
        <label className="settings-field">
          <span className="settings-label">{t("spending.appliesTo")}</span>
          <select
            className="settings-select"
            value={limit.scope}
            onChange={(e) => onChange({ ...limit, scope: e.currentTarget.value as LimitScope, target: "" })}
          >
            {scopeOptions().map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))}
          </select>
        </label>

        {limit.scope === "agent" && (
          <label className="settings-field">
            <span className="settings-label">{t("spending.agent")}</span>
            <select className="settings-select" value={limit.target} onChange={(e) => onChange({ ...limit, target: e.currentTarget.value })}>
              <option value="">{t("spending.pickAgent")}</option>
              {!knownAgent && limit.target !== "" && <option value={limit.target}>{t("spending.agentGone", { id: limit.target })}</option>}
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.id}
                </option>
              ))}
            </select>
          </label>
        )}
        {limit.scope === "channel" && (
          <label className="settings-field">
            <span className="settings-label">{t("spending.channel")}</span>
            <select className="settings-select" value={limit.target} onChange={(e) => onChange({ ...limit, target: e.currentTarget.value })}>
              <option value="">{t("spending.pickChannel")}</option>
              {!knownChannel && limit.target !== "" && <option value={limit.target}>{limit.target}</option>}
              {CHANNELS.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </select>
          </label>
        )}
        {limit.scope === "person" && (
          <label className="settings-field">
            <span className="settings-label">{t("spending.member")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="ana"
              value={limit.target}
              onChange={(e) => onChange({ ...limit, target: e.currentTarget.value })}
            />
            <span className="settings-hint">{t("spending.memberHint")}</span>
          </label>
        )}
        {limit.scope === "user" && (
          <label className="settings-field">
            <span className="settings-label">{t("spending.personLabel")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="telegram:12345"
              value={limit.target}
              onChange={(e) => onChange({ ...limit, target: e.currentTarget.value })}
            />
          </label>
        )}

        <NumberField label={t("spending.windowHours")} integer value={limit.windowHours} onChange={(n) => onChange({ ...limit, windowHours: n ?? 0 })} />
        <NumberField label={t("spending.maxTokens")} integer placeholder={t("spending.noCap")} value={limit.maxTokens} onChange={(n) => onChange({ ...limit, maxTokens: n })} />
        <NumberField label={t("spending.maxCost")} placeholder={t("spending.noCap")} value={limit.maxCostUsd} onChange={(n) => onChange({ ...limit, maxCostUsd: n })} />
        <PercentField label={t("spending.warnAt")} placeholder="80" value={limit.warnAt} onChange={(n) => onChange({ ...limit, warnAt: n })} />
        <PercentField label={t("spending.extendStep")} placeholder="25" value={limit.extendStep} onChange={(n) => onChange({ ...limit, extendStep: n })} />
      </div>
      <p className="settings-hint">{describeLimit(limit)}</p>
    </div>
  );
}

function PriceRow({
  price,
  onChange,
  onDelete,
}: {
  price: PriceEntry;
  onChange: (next: PriceEntry) => void;
  onDelete: () => void;
}) {
  return (
    <div className="spend-price-row">
      <input
        className="settings-input"
        type="text"
        list="spend-model-suggestions"
        placeholder={t("spending.modelId")}
        aria-label={t("spending.modelIdLabel")}
        value={price.model}
        onChange={(e) => onChange({ ...price, model: e.currentTarget.value })}
      />
      {/* A blank or unreadable price is kept as NaN so `validateSpending` names it on save; the field
          itself sees it as "no number" (NaN never equals itself, which would fight the resync). */}
      <NumberField
        label={t("spending.inputPrice")}
        ariaLabel={t("spending.inputPriceFor", { model: price.model || t("spending.thisModel") })}
        value={Number.isNaN(price.inputPerMtok) ? null : price.inputPerMtok}
        onChange={(n) => onChange({ ...price, inputPerMtok: n ?? NaN })}
      />
      <NumberField
        label={t("spending.outputPrice")}
        ariaLabel={t("spending.outputPriceFor", { model: price.model || t("spending.thisModel") })}
        value={Number.isNaN(price.outputPerMtok) ? null : price.outputPerMtok}
        onChange={(n) => onChange({ ...price, outputPerMtok: n ?? NaN })}
      />
      <button type="button" className="provider-delete-btn" onClick={onDelete} aria-label={t("spending.deletePriceFor", { model: price.model || t("spending.thisModel") })} title={t("spending.deletePrice")}>
        🗑
      </button>
    </div>
  );
}

/** Mirrors `spend_cmds` (the backend stays the authority) so a mistake is caught before the IPC
 * round-trip and named the way the form names things. `null` = fine. */
export function validateSpending(limits: LimitEntry[] | null, prices: PriceEntry[]): string | null {
  const ids = new Set<string>();
  for (const l of limits ?? []) {
    const id = l.id.trim();
    if (id === "") return t("spending.errNeedName");
    if (ids.has(id)) return t("spending.errDuplicate", { id });
    ids.add(id);
    if (!(l.windowHours >= 1)) return t("spending.errWindow", { id });
    if (l.maxTokens === null && l.maxCostUsd === null) return t("spending.errCaps", { id });
    if (l.maxTokens !== null && !(l.maxTokens > 0)) return t("spending.errTokens", { id });
    if (l.maxCostUsd !== null && !(l.maxCostUsd > 0)) return t("spending.errCost", { id });
    if (l.scope !== "global" && l.target.trim() === "") return t("spending.errTarget", { id });
    if (l.scope === "user" && !l.target.includes(":")) return t("spending.errUser", { id });
    if (l.warnAt !== null && !(l.warnAt > 0 && l.warnAt <= 1)) return t("spending.errWarn", { id });
    if (l.extendStep !== null && !(l.extendStep > 0)) return t("spending.errExtend", { id });
  }
  const models = new Set<string>();
  for (const p of prices) {
    const model = p.model.trim();
    if (model === "") return t("spending.errPriceModel");
    if (models.has(model)) return t("spending.errPriceDuplicate", { model });
    models.add(model);
    if (!(p.inputPerMtok >= 0) || !(p.outputPerMtok >= 0)) return t("spending.errPriceNumbers", { model });
  }
  return null;
}

export default function SpendingSection({
  limits,
  defaultLimits,
  disabledByEnv,
  prices,
  agents,
  modelSuggestions,
  onLimitsChange,
  onPricesChange,
}: {
  limits: LimitEntry[] | null;
  defaultLimits: LimitEntry[];
  disabledByEnv: boolean;
  prices: PriceEntry[];
  agents: AgentEntry[];
  /** Model ids already known from the providers, offered while typing a price's model. */
  modelSuggestions: string[];
  onLimitsChange: (next: LimitEntry[] | null) => void;
  onPricesChange: (next: PriceEntry[]) => void;
}) {
  const addLimit = () =>
    onLimitsChange([
      ...(limits ?? []),
      { id: nextLimitId(limits ?? []), scope: "global", target: "", windowHours: 24, maxTokens: null, maxCostUsd: null, warnAt: null, extendStep: null },
    ]);

  return (
    <section className="settings-section">
      <div className="settings-section-header">
        <h3 className="settings-section-title">{t("limits.title")}</h3>
        {limits !== null && (
          <button type="button" className="settings-browse-btn" onClick={addLimit}>
            {t("spending.addLimit")}
          </button>
        )}
      </div>
      <p className="settings-hint">{t("spending.hint")}</p>

      {disabledByEnv && <p className="settings-warning-banner">{rich("spending.disabledByEnv", { flag: <code>WARDEN_SPEND_LIMITS=off</code> })}</p>}

      {limits === null && (
        <div className="provider-card">
          <p className="settings-label">{t("spending.netOn")}</p>
          <ul className="spend-net-list">
            {defaultLimits.map((l) => (
              <li key={l.id} className="settings-hint">
                {describeLimit(l)}
              </li>
            ))}
          </ul>
          <div className="mcp-quick-add-row">
            <button type="button" className="settings-browse-btn" onClick={() => onLimitsChange(defaultLimits.map((l) => ({ ...l })))}>
              {t("spending.customize")}
            </button>
            <button type="button" className="settings-browse-btn" onClick={() => onLimitsChange([])}>
              {t("spending.allOff")}
            </button>
          </div>
        </div>
      )}

      {limits !== null && limits.length === 0 && <p className="settings-warning-banner">{t("spending.noneActive")}</p>}

      {limits !== null && (
        <>
          <div className="provider-list">
            {limits.map((l, i) => (
              <LimitCard
                key={i}
                limit={l}
                agents={agents}
                onChange={(next) => onLimitsChange(limits.map((x, j) => (j === i ? next : x)))}
                onDelete={() => onLimitsChange(limits.filter((_, j) => j !== i))}
              />
            ))}
          </div>
          <div className="mcp-quick-add-row">
            <button type="button" className="settings-browse-btn" onClick={() => onLimitsChange(null)}>
              {t("spending.restoreNet")}
            </button>
          </div>
        </>
      )}

      <div className="settings-section-header">
        <h4 className="settings-label">{t("spending.prices")}</h4>
        <button type="button" className="settings-browse-btn" onClick={() => onPricesChange([...prices, { model: "", inputPerMtok: 0, outputPerMtok: 0 }])}>
          {t("spending.addPrice")}
        </button>
      </div>
      <p className="settings-hint">{t("spending.pricesHint")}</p>
      <datalist id="spend-model-suggestions">
        {modelSuggestions.map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
      {prices.length === 0 && <p className="settings-hint">{t("spending.noPrices")}</p>}
      <div className="provider-list">
        {prices.map((p, i) => (
          <PriceRow
            key={i}
            price={p}
            onChange={(next) => onPricesChange(prices.map((x, j) => (j === i ? next : x)))}
            onDelete={() => onPricesChange(prices.filter((_, j) => j !== i))}
          />
        ))}
      </div>
    </section>
  );
}
