import { t } from "../i18n";
import { CEILINGS, type AdvancedDraft } from "./machineDraft";
import { Field, Section } from "./settingsParts";

// "Advanced" (P119): how far the assistant's sub-agents may go, and the TruthID sign-in. None of it
// reaches the machine or holds a secret, so it saves like the rest of the screen.

export default function AdvancedSection({ draft, error, onChange }: { draft: AdvancedDraft; error: string | null; onChange: (change: Partial<AdvancedDraft>) => void }) {
  return (
    <Section title={t("advanced.title")} hint={t("advanced.hint")}>
      {error && <p className="error-banner">{error}</p>}
      <p className="banner settings-note">{t("advanced.note", { depth: CEILINGS.depth, calls: CEILINGS.calls, jobs: CEILINGS.jobs })}</p>
      <div className="settings-grid">
        <Field label={t("advanced.depth")} hint={t("advanced.depthHint", { max: CEILINGS.depth })}>
          <input type="number" min={0} max={CEILINGS.depth} step={1} value={draft.depth} placeholder={t("advanced.default")} onChange={(e) => onChange({ depth: e.target.value })} />
        </Field>
        <Field label={t("advanced.calls")} hint={t("advanced.callsHint", { max: CEILINGS.calls })}>
          <input type="number" min={1} max={CEILINGS.calls} step={1} value={draft.calls} placeholder={t("advanced.default")} onChange={(e) => onChange({ calls: e.target.value })} />
        </Field>
        <Field label={t("advanced.jobs")} hint={t("advanced.jobsHint", { max: CEILINGS.jobs })}>
          <input type="number" min={0} max={CEILINGS.jobs} step={1} value={draft.jobs} placeholder={t("advanced.default")} onChange={(e) => onChange({ jobs: e.target.value })} />
        </Field>
        <Field label={t("advanced.network")}>
          <select value={draft.network} onChange={(e) => onChange({ network: e.target.value })}>
            <option value="base-mainnet">{t("advanced.netMain")}</option>
            <option value="base-sepolia">{t("advanced.netTest")}</option>
          </select>
        </Field>
        <Field label={t("advanced.rpc")} hint={t("advanced.rpcHint")} wide>
          <input value={draft.rpcUrl} onChange={(e) => onChange({ rpcUrl: e.target.value })} />
        </Field>
        <Field label={t("advanced.publicUrl")} hint={t("advanced.publicUrlHint")} wide>
          <input value={draft.publicUrl} placeholder="https://hub.example.com" onChange={(e) => onChange({ publicUrl: e.target.value })} />
        </Field>
      </div>
    </Section>
  );
}
