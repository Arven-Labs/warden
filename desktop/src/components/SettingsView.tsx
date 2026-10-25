import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { t } from "../i18n";
import { rich } from "../i18n/rich";
import { MCP_CATALOG, MCP_NEEDS_TEXT, catalogServer, catalogState } from "../lib/mcpCatalog";
import { isMcpServerHttp } from "../types";
import type { AgentEntry, Combo, GitSyncConfig, McpServer, ModelPolicy, OutreachEntry, ProviderEntry, ProviderKind, Settings, SshHostEntry, ToolCategoryEntry } from "../types";
import ApiKeysSection from "./ApiKeysSection";
import BotsSection from "./BotsSection";
import LanguageSwitch from "./LanguageSwitch";
import ThemeSwitch from "./ThemeSwitch";
import SshAuditLog from "./SshAuditLog";
import SpendingSection, { validateSpending } from "./SpendingSection";
import { APPROVAL_CATEGORIES } from "../lib/approvalCategories";
import { descendantsOf, removeFromOrg, renameInReports } from "../lib/org";
import { delegationCandidates, delegationSummary, dropModel, dropPolicy, nextPolicyId, renameModel, renamePolicy, type PolicyForm } from "../lib/modelPolicies";
import { dropOutreach, OUTREACH_CHANNELS, outreachForward, outreachOn, renameOutreach, setForward, setOutreach } from "../lib/outreach";

const emptySettings: Settings = {
  providers: [],
  activeProvider: "",
  combos: [],
  modelPolicies: [],
  outreach: [],
  toolCategories: [],
  persistGrants: true,
  grantTtlDays: 0,
  vaultPath: "",
  generatedPath: "",
  tavilyKey: "",
  whisperKey: "",
  enableShell: false,
  defaultModels: {},
  mcpServers: [],
  agents: [],
  sshHosts: [],
  gitSync: null,
  limits: null,
  defaultLimits: [],
  limitsDisabledByEnv: false,
  prices: [],
  version: "",
};

/** A combo's providers (P90), in the order they're tried when one is down (429, 5xx, no
 * connection). Each entry is a provider id; the picker only offers ones not already listed.
 * Also the ordered list of the models an agent may delegate with (P123): `candidates` then
 * names what can be picked, and `emptyHint` and `addLabel` say what the list is. */
function ProviderOrderEditor({
  value,
  providers = [],
  candidates,
  emptyHint,
  addLabel,
  onChange,
}: {
  value: string[];
  providers?: ProviderEntry[];
  candidates?: string[];
  emptyHint?: string;
  addLabel?: string;
  onChange: (next: string[]) => void;
}) {
  emptyHint ??= t("st.fallbackEmpty");
  addLabel ??= t("st.fallbackAdd");
  const available = (candidates ?? providers.map((p) => p.id)).filter((id) => id !== "" && !value.includes(id));

  function move(index: number, delta: number) {
    const next = [...value];
    const [item] = next.splice(index, 1);
    next.splice(index + delta, 0, item);
    onChange(next);
  }

  return (
    <div className="fallback-list">
      {value.length === 0 && <p className="settings-hint">{emptyHint}</p>}
      {value.map((id, index) => (
        <div key={id} className="fallback-row">
          <span className="fallback-order">{index + 1}.</span>
          <span className="fallback-name">{id}</span>
          <button type="button" className="provider-delete-btn" disabled={index === 0} onClick={() => move(index, -1)} title={t("st.moveUp")} aria-label={t("st.moveUpLabel", { id })}>
            ↑
          </button>
          <button
            type="button"
            className="provider-delete-btn"
            disabled={index === value.length - 1}
            onClick={() => move(index, 1)}
            title={t("st.moveDown")}
            aria-label={t("st.moveDownLabel", { id })}
          >
            ↓
          </button>
          <button type="button" className="provider-delete-btn" onClick={() => onChange(value.filter((v) => v !== id))} title={t("common.remove")} aria-label={t("st.removeLabel", { id })}>
            ✕
          </button>
        </div>
      ))}
      {available.length > 0 && (
        <select
          className="settings-select"
          value=""
          onChange={(e) => {
            if (e.currentTarget.value) onChange([...value, e.currentTarget.value]);
          }}
        >
          <option value="">{addLabel}</option>
          {available.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </select>
      )}
    </div>
  );
}

const emptyGitSync: GitSyncConfig = { remoteUrl: "", token: "" };

/** Connection form for the git sync backend (P63/P71 v2) — a self-hosted/remote git repo as an
 * alternative to Arweave for the Sync screen's push/pull (and the auto-sync loop). The vault itself
 * always lives on this machine (P61). `value` is `null` until the user starts typing; `onChange`
 * always passes a complete object back so partial edits never get lost between keystrokes. */
function GitSyncForm({ value, onChange }: { value: GitSyncConfig | null; onChange: (next: GitSyncConfig) => void }) {
  const current = value ?? emptyGitSync;

  function set<K extends keyof GitSyncConfig>(key: K, v: GitSyncConfig[K]) {
    onChange({ ...current, [key]: v });
  }

  return (
    <div className="git-sync-form">
      <label className="settings-field">
        <span className="settings-label">{t("st.git.remoteUrl")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder="https://gitea.example.com/user/vault.git"
          value={current.remoteUrl}
          onChange={(e) => set("remoteUrl", e.currentTarget.value)}
        />
      </label>
      <ApiKeyField label={t("st.git.token")} value={current.token} onChange={(v) => set("token", v)} />
    </div>
  );
}

const providerKindOptions = (): { value: ProviderKind; label: string }[] => [
  { value: "gemini", label: "Gemini" },
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "openai_compatible", label: t("st.kind.openaiCompatible") },
  { value: "node", label: t("st.kind.node") },
  { value: "cloud", label: t("st.kind.cloud") },
];

/** Reused outside this file too (e.g. `WorkspaceView.tsx`'s embedded-server auth key) — same
 * reveal/hide affordance for any secret the desktop shows in an editable field. */
export function ApiKeyField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [revealed, setRevealed] = useState(false);

  return (
    <label className="settings-field">
      <span className="settings-label">{label}</span>
      <div className="settings-key-field">
        <input
          className="settings-input"
          type={revealed ? "text" : "password"}
          placeholder={t("st.notSet")}
          value={value}
          onChange={(e) => onChange(e.currentTarget.value)}
          autoComplete="off"
        />
        <button
          type="button"
          className="settings-key-toggle"
          aria-label={revealed ? t("st.hide", { label }) : t("st.reveal", { label })}
          onClick={() => setRevealed((r) => !r)}
        >
          {revealed ? "🙈" : "👁"}
        </button>
      </div>
    </label>
  );
}

/** First name not already used by another provider — "provider-2", "provider-3", etc. */
function nextProviderId(existing: ProviderEntry[]): string {
  let n = existing.length + 1;
  while (existing.some((p) => p.id === `provider-${n}`)) {
    n += 1;
  }
  return `provider-${n}`;
}

function ProviderCard({
  provider,
  isActive,
  defaultModel,
  onChange,
  onDelete,
  onSetActive,
}: {
  provider: ProviderEntry;
  isActive: boolean;
  defaultModel: string | undefined;
  onChange: (next: ProviderEntry) => void;
  onDelete: () => void;
  onSetActive: () => void;
}) {
  const isOpenAiCompatible = provider.kind === "openai_compatible";
  const isNode = provider.kind === "node";
  /** P10 — "Test key": asks the provider for its model list with the key as typed (saved or not). */
  const [testing, setTesting] = useState(false);
  const [keyTest, setKeyTest] = useState<{ ok: boolean; kind: string; message: string } | null>(null);

  async function testKey() {
    setTesting(true);
    setKeyTest(null);
    try {
      setKeyTest(await invoke<{ ok: boolean; kind: string; message: string }>("test_provider_key", { provider }));
    } catch (err) {
      setKeyTest({ ok: false, kind: "unreachable", message: String(err) });
    } finally {
      setTesting(false);
    }
  }

  return (
    <div className={`provider-card${isActive ? " provider-card-active" : ""}`}>
      <div className="provider-card-header">
        <label className="provider-active-toggle" title={t("st.useProvider")}>
          <input type="radio" name="active-provider" checked={isActive} onChange={onSetActive} />
          <span>{t("st.active")}</span>
        </label>
        <input
          className="settings-input provider-name-input"
          type="text"
          placeholder={t("st.provider.namePlaceholder")}
          value={provider.id}
          onChange={(e) => onChange({ ...provider, id: e.currentTarget.value })}
        />
        <button
          type="button"
          className="provider-delete-btn"
          onClick={onDelete}
          aria-label={t("st.provider.deleteLabel", { name: provider.id || t("st.provider.thisProvider") })}
          title={t("st.provider.deleteTitle")}
        >
          🗑
        </button>
      </div>

      <label className="settings-field">
        <span className="settings-label">{t("st.provider.type")}</span>
        <select
          className="settings-select"
          value={provider.kind}
          onChange={(e) => onChange({ ...provider, kind: e.currentTarget.value as ProviderKind })}
        >
          {providerKindOptions().map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </label>

      {isOpenAiCompatible && (
        <label className="settings-field">
          <span className="settings-label">{t("st.provider.baseUrl")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder="http://localhost:11434/v1"
            value={provider.baseUrl}
            onChange={(e) => onChange({ ...provider, baseUrl: e.currentTarget.value })}
          />
        </label>
      )}

      {provider.kind === "cloud" && (
        <label className="settings-field">
          <span className="settings-label">{t("st.provider.baseUrlCloud")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder="https://my-hub.example/v1"
            value={provider.baseUrl}
            onChange={(e) => onChange({ ...provider, baseUrl: e.currentTarget.value })}
          />
        </label>
      )}

      {isNode && (
        <label className="settings-field">
          <span className="settings-label">{t("st.provider.nodeId")}</span>
          <input
            className="settings-input"
            type="text"
            placeholder="node-home-pc-1a2b3c4d"
            value={provider.node ?? ""}
            onChange={(e) => onChange({ ...provider, node: e.currentTarget.value })}
          />
          <span className="settings-hint">{t("st.provider.nodeHint")}</span>
        </label>
      )}

      {!isNode && (
        <ApiKeyField
          label={isOpenAiCompatible ? t("st.provider.apiKeyOptional") : t("st.provider.apiKey")}
          value={provider.apiKey}
          onChange={(v) => onChange({ ...provider, apiKey: v })}
        />
      )}

      {!isNode && (
        <div className="settings-field">
          <button type="button" className="settings-browse-btn" disabled={testing} onClick={() => void testKey()}>
            {testing ? t("st.provider.testing") : t("st.provider.test")}
          </button>
          <span className="settings-hint">{t("st.provider.testHint")}</span>
          {keyTest && (
            <div
              role="status"
              className={keyTest.ok ? "settings-success-banner" : keyTest.kind === "unverifiable" || keyTest.kind === "rate_limited" ? "settings-hint" : "settings-error-banner"}
            >
              {keyTest.ok ? "✓ " : keyTest.kind === "unverifiable" || keyTest.kind === "rate_limited" ? "⚠ " : "✗ "}
              {keyTest.message}
            </div>
          )}
        </div>
      )}

      <label className="settings-field">
        <span className="settings-label">{isNode ? t("st.provider.onNode") : t("st.provider.model")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder={isNode ? t("st.provider.nodeModelPlaceholder") : defaultModel ? t("st.provider.defaultModel", { model: defaultModel }) : isOpenAiCompatible ? t("st.provider.requiredModel") : ""}
          value={provider.model}
          onChange={(e) => onChange({ ...provider, model: e.currentTarget.value })}
        />
      </label>
    </div>
  );
}

/** First name not already used by another agent — "agent-2", "agent-3", etc., same scheme as
 * `nextProviderId`. */
function nextAgentId(existing: AgentEntry[]): string {
  let n = existing.length + 1;
  while (existing.some((a) => a.id === `agent-${n}`)) {
    n += 1;
  }
  return `agent-${n}`;
}

function nextSshHostId(existing: SshHostEntry[]): string {
  let n = existing.length + 1;
  while (existing.some((h) => h.id === `server-${n}`)) {
    n += 1;
  }
  return `server-${n}`;
}

function SshHostCard({
  host,
  agents,
  onChange,
  onDelete,
}: {
  host: SshHostEntry;
  agents: AgentEntry[];
  onChange: (next: SshHostEntry) => void;
  onDelete: () => void;
}) {
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);

  async function testConnection() {
    setTesting(true);
    setTestResult(null);
    try {
      setTestResult(await invoke<{ ok: boolean; message: string }>("test_ssh_host", { host }));
    } catch (err) {
      setTestResult({ ok: false, message: String(err) });
    } finally {
      setTesting(false);
    }
  }

  function toggleAgent(id: string, checked: boolean) {
    onChange({ ...host, agents: checked ? [...host.agents.filter((a) => a !== id), id] : host.agents.filter((a) => a !== id) });
  }

  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          placeholder={t("st.ssh.namePlaceholder")}
          value={host.id}
          onChange={(e) => onChange({ ...host, id: e.currentTarget.value })}
        />
        <button
          type="button"
          className="provider-delete-btn"
          onClick={onDelete}
          aria-label={t("st.provider.deleteLabel", { name: host.id || t("st.ssh.thisServer") })}
          title={t("st.ssh.deleteTitle")}
        >
          🗑
        </button>
      </div>

      <label className="settings-field">
        <span className="settings-label">{t("st.ssh.host")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder={t("st.ssh.hostPlaceholder")}
          value={host.host}
          onChange={(e) => onChange({ ...host, host: e.currentTarget.value })}
        />
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.ssh.user")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder="deploy"
          value={host.user}
          onChange={(e) => onChange({ ...host, user: e.currentTarget.value })}
        />
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.ssh.port")}</span>
        <input
          className="settings-input"
          type="number"
          min={1}
          max={65535}
          value={host.port}
          onChange={(e) => onChange({ ...host, port: Number(e.currentTarget.value) })}
        />
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.ssh.identity")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder="/home/you/.ssh/id_ed25519"
          value={host.identityFile}
          onChange={(e) => onChange({ ...host, identityFile: e.currentTarget.value })}
        />
        <span className="settings-hint">{t("st.ssh.identityHint")}</span>
      </label>

      <div className="settings-field">
        <span className="settings-label">{t("st.ssh.availableTo")}</span>
        {agents.length === 0 ? (
          <span className="settings-hint">{t("st.ssh.noAgents")}</span>
        ) : (
          <div className="skill-agent-list">
            {agents.map((a) => (
              <label className="skill-agent-option" key={a.id}>
                <input type="checkbox" checked={host.agents.includes(a.id)} onChange={(e) => toggleAgent(a.id, e.currentTarget.checked)} />
                {a.id}
              </label>
            ))}
          </div>
        )}
        <span className="settings-hint">{rich("st.ssh.availableHint", { chats: <strong>{t("st.ssh.andEveryChat")}</strong> })}</span>
      </div>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input type="checkbox" checked={host.enabled} onChange={(e) => onChange({ ...host, enabled: e.currentTarget.checked })} />
          <span className="settings-label">{t("st.ssh.enable")}</span>
        </span>
        <span className="settings-hint">{t("st.ssh.enableHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={host.requireApproval}
            onChange={(e) => onChange({ ...host, requireApproval: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.ssh.requireApproval")}</span>
        </span>
        <span className="settings-hint">{t("st.ssh.requireApprovalHint")}</span>
      </label>

      <div className="settings-field">
        <button type="button" className="settings-browse-btn" onClick={testConnection} disabled={testing}>
          {testing ? t("st.provider.testing") : t("st.ssh.test")}
        </button>
        {testResult && (
          <div className={testResult.ok ? "settings-success-banner" : "settings-error-banner"} role="status">
            {testResult.ok ? "✓ " : "✗ "}
            {testResult.message}
          </div>
        )}
      </div>
    </div>
  );
}

/** One combo (P90): its name (also its id), whether it's the active model, and its providers. */
function ComboCard({
  combo,
  providers,
  isActive,
  onChange,
  onDelete,
  onSetActive,
}: {
  combo: Combo;
  providers: ProviderEntry[];
  isActive: boolean;
  onChange: (next: Combo) => void;
  onDelete: () => void;
  onSetActive: () => void;
}) {
  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <label className="provider-active-toggle" title={t("st.combo.useIt")}>
          <input type="radio" name="active-provider" checked={isActive} onChange={onSetActive} />
          <span>{t("st.active")}</span>
        </label>
        <input
          className="settings-input provider-name-input"
          type="text"
          value={combo.id}
          placeholder={t("st.combo.name")}
          aria-label={t("st.combo.nameLabel")}
          onChange={(e) => onChange({ ...combo, id: e.currentTarget.value })}
        />
        <button type="button" className="provider-delete-btn" onClick={onDelete} aria-label={t("st.combo.deleteLabel", { name: combo.id || t("st.combo.thisCombo") })} title={t("st.combo.deleteTitle")}>
          ✕
        </button>
      </div>
      <ProviderOrderEditor value={combo.providers} providers={providers} onChange={(members) => onChange({ ...combo, providers: members })} />
    </div>
  );
}

/** One entry of the map of tools to risk categories (P122): the tool's name and the kind of action its calls are. */
function ToolCategoryRow({ entry, onChange, onDelete }: { entry: ToolCategoryEntry; onChange: (next: ToolCategoryEntry) => void; onDelete: () => void }) {
  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          value={entry.tool}
          placeholder={t("st.tool.name")}
          aria-label={t("st.tool.nameLabel")}
          onChange={(e) => onChange({ ...entry, tool: e.currentTarget.value })}
        />
        <select className="settings-select" value={entry.category} aria-label={t("st.tool.category")} onChange={(e) => onChange({ ...entry, category: e.currentTarget.value })}>
          {APPROVAL_CATEGORIES.map((c) => (
            <option key={c.id} value={c.id} title={c.hint}>
              {c.label}
            </option>
          ))}
        </select>
        <button type="button" className="provider-delete-btn" onClick={onDelete} aria-label={t("st.tool.removeLabel", { name: entry.tool || t("st.tool.thisTool") })} title={t("st.tool.removeTitle")}>
          ✕
        </button>
      </div>
    </div>
  );
}

/** One model policy (P123): its name (also its id), the provider or combo that answers it, and what an agent reads to know when to pick it. */
function PolicyCard({
  policy,
  modelChoices,
  onChange,
  onDelete,
}: {
  policy: ModelPolicy;
  /** The providers and combos that can answer it. */
  modelChoices: string[];
  onChange: (next: ModelPolicy) => void;
  onDelete: () => void;
}) {
  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          value={policy.id}
          placeholder={t("st.policy.name")}
          aria-label={t("st.policy.nameLabel")}
          onChange={(e) => onChange({ ...policy, id: e.currentTarget.value })}
        />
        <select className="settings-select" value={policy.model} aria-label={t("st.policy.modelLabel", { name: policy.id || t("st.policy.thisPolicy") })} onChange={(e) => onChange({ ...policy, model: e.currentTarget.value })}>
          {!modelChoices.includes(policy.model) && <option value={policy.model}>{policy.model || t("st.policy.pick")}</option>}
          {modelChoices.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </select>
        <button type="button" className="provider-delete-btn" onClick={onDelete} aria-label={t("st.policy.deleteLabel", { name: policy.id || t("st.policy.thisPolicy") })} title={t("st.policy.deleteTitle")}>
          ✕
        </button>
      </div>
      <label className="settings-field">
        <span className="settings-label">{t("st.policy.when")}</span>
        <input
          className="settings-input"
          type="text"
          maxLength={200}
          placeholder={t("st.policy.whenPlaceholder")}
          value={policy.description ?? ""}
          onChange={(e) => onChange({ ...policy, description: e.currentTarget.value })}
        />
      </label>
    </div>
  );
}

function AgentCard({
  agent,
  providers,
  combos,
  modelChoices,
  toolNames,
  people,
  allAgents,
  outreach,
  onOutreach,
  onChange,
  onDelete,
}: {
  agent: AgentEntry;
  /** P121 — the agents allowed to start messages, with the external channels each also reaches. */
  outreach: OutreachEntry[];
  onOutreach: (next: OutreachEntry[]) => void;
  /** Every agent of the owner's, for the "Reports to" list. */
  allAgents: AgentEntry[];
  providers: ProviderEntry[];
  combos: Combo[];
  /** The providers, combos and policies (P123) an agent may be limited to when it delegates. */
  modelChoices: string[];
  /** Every tool the running app has, for the "Restrict tools" list. */
  toolNames: string[];
  /** P84 — the workspace's members, to share this agent with. */
  people: { id: string; name: string }[];
  onChange: (next: AgentEntry) => void;
  onDelete: () => void;
}) {
  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          placeholder={t("st.agent.namePlaceholder")}
          value={agent.id}
          onChange={(e) => onChange({ ...agent, id: e.currentTarget.value })}
        />
        <button
          type="button"
          className="provider-delete-btn"
          onClick={onDelete}
          aria-label={t("st.provider.deleteLabel", { name: agent.id || t("st.agent.thisAgent") })}
          title={t("st.agent.deleteTitle")}
        >
          🗑
        </button>
      </div>

      <label className="settings-field">
        <span className="settings-label">{t("st.agent.role")}</span>
        <input
          className="settings-input"
          type="text"
          placeholder={t("st.agent.rolePlaceholder")}
          value={agent.role ?? ""}
          onChange={(e) => onChange({ ...agent, role: e.currentTarget.value })}
        />
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.agent.reportsTo")}</span>
        <select
          className="settings-select"
          value={agent.reportsTo ?? ""}
          onChange={(e) => onChange({ ...agent, reportsTo: e.currentTarget.value || null })}
        >
          <option value="">{t("st.agent.nobody")}</option>
          {allAgents
            .filter((other) => other.id !== agent.id && !descendantsOf(allAgents, agent.id).has(other.id))
            .map((other) => (
              <option key={other.id} value={other.id}>
                {other.id}
              </option>
            ))}
        </select>
        <span className="settings-hint">{t("st.agent.reportsHint")}</span>
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.agent.personality")}</span>
        <textarea
          className="settings-input settings-textarea"
          placeholder={t("st.agent.personalityPlaceholder")}
          rows={3}
          value={agent.persona}
          onChange={(e) => onChange({ ...agent, persona: e.currentTarget.value })}
        />
      </label>

      <label className="settings-field">
        <span className="settings-label">{t("st.agent.defaultModel")}</span>
        <select
          className="settings-select"
          value={agent.providerId}
          onChange={(e) => onChange({ ...agent, providerId: e.currentTarget.value })}
        >
          <option value="">{t("st.agent.useActive")}</option>
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.id}
            </option>
          ))}
          {combos.map((c) => (
            <option key={c.id} value={c.id}>
              {t("st.agent.comboOption", { id: c.id })}
            </option>
          ))}
        </select>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canDelegateToAgents}
            onChange={(e) => onChange({ ...agent, canDelegateToAgents: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canDelegate")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canDelegateHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canManageAgents}
            onChange={(e) => onChange({ ...agent, canManageAgents: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canManage")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canManageHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canMessageAgents}
            onChange={(e) => onChange({ ...agent, canMessageAgents: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canMessage")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canMessageHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canManageTasks}
            onChange={(e) => onChange({ ...agent, canManageTasks: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canTasks")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canTasksHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canStartTasks}
            onChange={(e) => onChange({ ...agent, canStartTasks: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canStart")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canStartHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canCreateWorkers}
            onChange={(e) => onChange({ ...agent, canCreateWorkers: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canWorkers")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canWorkersHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canMessageUser}
            onChange={(e) => onChange({ ...agent, canMessageUser: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canMessageUser")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canMessageUserHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.canChooseModels}
            onChange={(e) => onChange({ ...agent, canChooseModels: e.currentTarget.checked })}
          />
          <span className="settings-label">{t("st.agent.canChoose")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.canChooseHint")}</span>
      </label>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input type="checkbox" checked={outreachOn(outreach, agent.id)} onChange={(e) => onOutreach(setOutreach(outreach, agent.id, e.currentTarget.checked))} />
          <span className="settings-label">{t("st.agent.outreach")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.outreachHint")}</span>
      </label>
      {outreachOn(outreach, agent.id) && (
        <div className="settings-field">
          <span className="settings-label">{t("st.agent.alsoSend")}</span>
          {OUTREACH_CHANNELS.map((channel) => (
            <span key={channel} className="settings-checkbox-row">
              <input
                type="checkbox"
                checked={outreachForward(outreach, agent.id).includes(channel)}
                onChange={(e) => onOutreach(setForward(outreach, agent.id, channel, e.currentTarget.checked))}
              />
              <span className="settings-label">{channel === "telegram" ? "Telegram" : "WhatsApp"}</span>
            </span>
          ))}
          <span className="settings-hint">{t("st.agent.alsoSendHint")}</span>
        </div>
      )}

      <label className="settings-field">
        <span className="settings-label">{t("st.agent.autonomy")}</span>
        <select
          className="settings-input"
          value={agent.autonomy}
          onChange={(e) => onChange({ ...agent, autonomy: Number(e.currentTarget.value) })}
        >
          <option value={1}>{t("st.agent.autonomy1")}</option>
          <option value={2}>{t("st.agent.autonomy2")}</option>
          <option value={3}>{t("st.agent.autonomy3")}</option>
          <option value={4}>{t("st.agent.autonomy4")}</option>
          <option value={5}>{t("st.agent.autonomy5")}</option>
        </select>
        <span className="settings-hint">{t("st.agent.autonomyHint")}</span>
      </label>

      <div className="settings-field" role="group" aria-label={t("st.agent.mustApprove", { name: agent.id || t("st.agent.thisAgent") })}>
        <span className="settings-label">{t("st.agent.alwaysAsk")}</span>
        {APPROVAL_CATEGORIES.map((category) => (
          <span key={category.id} className="settings-checkbox-row">
            <input
              type="checkbox"
              id={`approval-${agent.id}-${category.id}`}
              checked={agent.approvalRequired.includes(category.id)}
              onChange={(e) =>
                onChange({
                  ...agent,
                  approvalRequired: e.currentTarget.checked
                    ? [...agent.approvalRequired, category.id]
                    : agent.approvalRequired.filter((id) => id !== category.id),
                })
              }
            />
            <label htmlFor={`approval-${agent.id}-${category.id}`} title={category.hint}>
              {category.label}
            </label>
          </span>
        ))}
        <span className="settings-hint">{t("st.agent.alwaysAskHint")}</span>
      </div>

      <label className="settings-field settings-checkbox-field">
        <span className="settings-checkbox-row">
          <input
            type="checkbox"
            checked={agent.allowedTools !== null}
            onChange={(e) => onChange({ ...agent, allowedTools: e.currentTarget.checked ? [] : null })}
          />
          <span className="settings-label">{t("st.agent.restrictTools")}</span>
        </span>
        <span className="settings-hint">{t("st.agent.restrictToolsHint")}</span>
      </label>
      {agent.allowedTools !== null && (
        <div className="settings-field agent-tool-list" role="group" aria-label={t("st.agent.toolsMayUse", { name: agent.id || t("st.agent.thisAgent") })}>
          {[...new Set([...toolNames, ...agent.allowedTools])].map((tool) => (
            <span key={tool} className="settings-checkbox-row">
              <input
                type="checkbox"
                id={`tool-${agent.id}-${tool}`}
                checked={agent.allowedTools!.includes(tool)}
                onChange={(e) =>
                  onChange({
                    ...agent,
                    allowedTools: e.currentTarget.checked
                      ? [...agent.allowedTools!, tool]
                      : agent.allowedTools!.filter((other) => other !== tool),
                  })
                }
              />
              <label htmlFor={`tool-${agent.id}-${tool}`}>
                {tool}
                {!toolNames.includes(tool) && <span className="settings-hint">{t("st.agent.notAvailable")}</span>}
              </label>
            </span>
          ))}
          {agent.allowedTools.length === 0 && <span className="settings-hint">{t("st.agent.noToolTicked")}</span>}
        </div>
      )}
      {(agent.canDelegateToAgents || agent.canManageAgents || modelChoices.length > 0) && (
        <div className="settings-field" role="group" aria-label={t("st.agent.delegationModels", { name: agent.id || t("st.agent.thisAgent") })}>
          <span className="settings-label">{t("st.agent.delegationTitle")}</span>
          <span className="settings-hint">{t("st.agent.delegationHint", { summary: delegationSummary(agent.delegationModels) })}</span>
          <ProviderOrderEditor
            value={agent.delegationModels ?? []}
            candidates={modelChoices}
            emptyHint={t("st.agent.delegationEmpty")}
            addLabel={t("st.agent.delegationAdd")}
            onChange={(next) => onChange({ ...agent, delegationModels: next })}
          />
        </div>
      )}
      {people.length > 0 && (
        <div className="settings-field agent-tool-list" role="group" aria-label={t("st.agent.sharedWithLabel", { name: agent.id || t("st.agent.thisAgent") })}>
          <span className="settings-label">{t("st.agent.sharedWith")}</span>
          <span className="settings-hint">{t("st.agent.sharedHint")}</span>
          <span className="settings-checkbox-row">
            <input
              type="checkbox"
              id={`share-${agent.id}-all`}
              checked={(agent.sharedWith ?? []).includes("*")}
              onChange={(e) => onChange({ ...agent, sharedWith: e.currentTarget.checked ? ["*"] : [] })}
            />
            <label htmlFor={`share-${agent.id}-all`}>{t("st.agent.everyone")}</label>
          </span>
          {!(agent.sharedWith ?? []).includes("*") &&
            people.map((p) => (
              <span key={p.id} className="settings-checkbox-row">
                <input
                  type="checkbox"
                  id={`share-${agent.id}-${p.id}`}
                  checked={(agent.sharedWith ?? []).includes(p.id)}
                  onChange={(e) =>
                    onChange({
                      ...agent,
                      sharedWith: e.currentTarget.checked ? [...(agent.sharedWith ?? []), p.id] : (agent.sharedWith ?? []).filter((s) => s !== p.id),
                    })
                  }
                />
                <label htmlFor={`share-${agent.id}-${p.id}`}>
                  {p.name} ({p.id})
                </label>
              </span>
            ))}
        </div>
      )}
    </div>
  );
}

/** Editor for a `Record<string, string>` shown as key/value rows — shared by env vars (stdio
 * servers) and headers (HTTP servers), the only two places this shape shows up. */
function KeyValueListField({
  label,
  addLabel,
  keyPlaceholder,
  entries,
  onChange,
}: {
  label: string;
  addLabel: string;
  keyPlaceholder: string;
  entries: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
}) {
  const list = Object.entries(entries);

  function updateEntry(index: number, key: string, value: string) {
    const next = list.map((entry, i) => (i === index ? ([key, value] as [string, string]) : entry));
    onChange(Object.fromEntries(next));
  }

  function addEntry() {
    // A blank key is a valid (if temporary) React list item — it's the value being edited that
    // matters; a truly empty key just gets dropped on save the same way an empty provider name
    // would be, so nothing needs deduping here.
    onChange({ ...entries, "": "" });
  }

  function removeEntry(index: number) {
    onChange(Object.fromEntries(list.filter((_, i) => i !== index)));
  }

  return (
    <div className="settings-field">
      <span className="settings-label">{label}</span>
      {list.map(([key, value], i) => (
        <div className="env-var-row" key={i}>
          <input
            className="settings-input"
            type="text"
            placeholder={keyPlaceholder}
            value={key}
            onChange={(e) => updateEntry(i, e.currentTarget.value, value)}
          />
          <input
            className="settings-input"
            type="password"
            placeholder={t("st.kv.value")}
            value={value}
            onChange={(e) => updateEntry(i, key, e.currentTarget.value)}
          />
          <button type="button" className="provider-delete-btn" onClick={() => removeEntry(i)} aria-label={t("st.kv.removeLabel", { name: key || t("st.kv.thisEntry") })}>
            ✕
          </button>
        </div>
      ))}
      <button type="button" className="settings-browse-btn" onClick={addEntry}>
        + {addLabel}
      </button>
    </div>
  );
}

/** Connect/Disconnect UI for an OAuth-authenticated MCP server (PENDING.md P26) — status comes
 * only from whether a token file exists locally (no network call, see `mcp_oauth_status`'s doc
 * comment); a token that's actually expired/unrefreshable only surfaces the next time the
 * orchestrator tries to use that server. Independent of the form's save state on purpose: a
 * "Connect" click persists the token immediately, whether or not this card's been saved yet — a
 * later Save (or the reconnect this panel already triggers) is what makes the orchestrator
 * actually pick the server up. */
function McpOAuthPanel({ name, url }: { name: string; url: string }) {
  const [connected, setConnected] = useState<boolean | null>(null);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  function refreshStatus() {
    if (!name.trim()) {
      setConnected(false);
      return;
    }
    invoke<boolean>("mcp_oauth_status", { name })
      .then(setConnected)
      .catch(() => setConnected(false));
  }

  useEffect(refreshStatus, [name]);

  async function handleConnect() {
    setError(null);
    setIsBusy(true);
    try {
      await invoke("mcp_oauth_connect", { name, url });
      refreshStatus();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  async function handleDisconnect() {
    setError(null);
    setIsBusy(true);
    try {
      await invoke("mcp_oauth_disconnect", { name });
      refreshStatus();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsBusy(false);
    }
  }

  const canConnect = name.trim() !== "" && url.trim() !== "";

  return (
    <div className="settings-field">
      <span className="settings-label">{t("st.oauth.title")}</span>
      <div className="settings-key-field">
        <span className={`mcp-oauth-status${connected ? " mcp-oauth-status-connected" : ""}`}>
          {connected === null ? t("st.oauth.checking") : connected ? t("st.oauth.connected") : t("st.oauth.notConnected")}
        </span>
        {connected ? (
          <button type="button" className="settings-browse-btn" onClick={handleDisconnect} disabled={isBusy}>
            {isBusy ? t("st.oauth.disconnecting") : t("st.oauth.disconnect")}
          </button>
        ) : (
          <button type="button" className="settings-browse-btn" onClick={handleConnect} disabled={isBusy || !canConnect}>
            {isBusy ? t("st.oauth.opening") : t("st.oauth.connect")}
          </button>
        )}
      </div>
      {!canConnect && <span className="settings-hint">{t("st.oauth.required")}</span>}
      {error && <p className="settings-error-banner">{error}</p>}
    </div>
  );
}

function McpServerCard({
  server,
  onChange,
  onDelete,
}: {
  server: McpServer;
  onChange: (next: McpServer) => void;
  onDelete: () => void;
}) {
  const isHttp = isMcpServerHttp(server);

  function setTransport(next: "stdio" | "http") {
    if (next === (isHttp ? "http" : "stdio")) return;
    onChange(next === "http" ? { name: server.name, url: "", headers: {}, oauth: false } : { name: server.name, command: "", args: [], env: {} });
  }

  function updateArgs(text: string) {
    const args = text
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
    onChange({ ...server, args } as McpServer);
  }

  return (
    <div className="provider-card">
      <div className="provider-card-header">
        <input
          className="settings-input provider-name-input"
          type="text"
          placeholder={t("st.mcp.namePlaceholder")}
          value={server.name}
          onChange={(e) => onChange({ ...server, name: e.currentTarget.value })}
        />
        <button
          type="button"
          className="provider-delete-btn"
          onClick={onDelete}
          aria-label={t("st.provider.deleteLabel", { name: server.name || t("st.mcp.thisServer") })}
          title={t("st.mcp.deleteTitle")}
        >
          🗑
        </button>
      </div>

      <label className="settings-field">
        <span className="settings-label">{t("st.mcp.transport")}</span>
        <select className="settings-select" value={isHttp ? "http" : "stdio"} onChange={(e) => setTransport(e.currentTarget.value as "stdio" | "http")}>
          <option value="stdio">{t("st.mcp.stdio")}</option>
          <option value="http">{t("st.mcp.http")}</option>
        </select>
      </label>

      {isHttp ? (
        <>
          <label className="settings-field">
            <span className="settings-label">{t("st.mcp.url")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="https://mcp.example.com/mcp"
              value={server.url}
              onChange={(e) => onChange({ ...server, url: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field settings-checkbox-field">
            <span className="settings-checkbox-row">
              <input type="checkbox" checked={server.oauth} onChange={(e) => onChange({ ...server, oauth: e.currentTarget.checked })} />
              <span className="settings-label">{t("st.mcp.oauth")}</span>
            </span>
            <span className="settings-hint">{t("st.mcp.oauthHint")}</span>
          </label>

          {server.oauth ? (
            <McpOAuthPanel name={server.name} url={server.url} />
          ) : (
            <KeyValueListField
              label={t("st.mcp.headers")}
              addLabel={t("st.mcp.addHeader")}
              keyPlaceholder="Authorization"
              entries={server.headers}
              onChange={(headers) => onChange({ ...server, headers })}
            />
          )}
        </>
      ) : (
        <>
          <label className="settings-field">
            <span className="settings-label">{t("st.mcp.command")}</span>
            <input
              className="settings-input"
              type="text"
              placeholder="npx"
              value={server.command}
              onChange={(e) => onChange({ ...server, command: e.currentTarget.value })}
            />
          </label>

          <label className="settings-field">
            <span className="settings-label">{t("st.mcp.args")}</span>
            <textarea
              className="settings-input settings-textarea"
              rows={3}
              placeholder={"-y\n@notionhq/notion-mcp-server"}
              value={server.args.join("\n")}
              onChange={(e) => updateArgs(e.currentTarget.value)}
            />
          </label>

          <KeyValueListField
            label={t("st.mcp.env")}
            addLabel={t("st.mcp.addVariable")}
            keyPlaceholder="KEY"
            entries={server.env}
            onChange={(env) => onChange({ ...server, env })}
          />
        </>
      )}
    </div>
  );
}

function SettingsView() {
  const [form, setForm] = useState<Settings>(emptySettings);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [toolNames, setToolNames] = useState<string[]>([]);
  const [people, setPeople] = useState<{ id: string; name: string }[]>([]);

  useEffect(() => {
    invoke<string[]>("list_tool_names")
      .then(setToolNames)
      .catch(() => setToolNames([]));
    invoke<{ users: { id: string; name: string }[] }>("list_people")
      .then((p) => setPeople(p.users))
      .catch(() => setPeople([]));
    invoke<Settings>("get_settings")
      .then(setForm)
      .catch((err) => setError(String(err)))
      .finally(() => setIsLoading(false));
  }, []);

  async function handleBrowseVaultPath() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setForm((f) => ({ ...f, vaultPath: selected }));
    }
  }

  async function handleBrowseGeneratedPath() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setForm((f) => ({ ...f, generatedPath: selected }));
    }
  }

  function addProvider() {
    setForm((f) => {
      const id = nextProviderId(f.providers);
      const entry: ProviderEntry = { id, kind: "gemini", apiKey: "", baseUrl: "", model: "" };
      return { ...f, providers: [...f.providers, entry], activeProvider: f.activeProvider || id };
    });
  }

  function updateProvider(index: number, next: ProviderEntry) {
    setForm((f) => {
      const prevId = f.providers[index]?.id;
      const providers = f.providers.map((p, i) => (i === index ? next : p));
      if (prevId === undefined || prevId === next.id) {
        return { ...f, providers };
      }
      // Renaming a provider's id (the "Name" field doubles as its id, same scheme as
      // nextProviderId) must carry the rename to whatever else in this form referenced the old
      // id — otherwise "Active" and any agent's default model silently point at a name that no
      // longer exists, and resolving it later fails with a raw "not found" error.
      const activeProvider = f.activeProvider === prevId ? next.id : f.activeProvider;
      const agents = f.agents.map((a) => (a.providerId === prevId ? { ...a, providerId: next.id } : a));
      const combos = f.combos.map((c) => ({ ...c, providers: c.providers.map((id) => (id === prevId ? next.id : id)) }));
      return renameModel({ ...f, providers, activeProvider, agents, combos }, prevId, next.id);
    });
  }

  function deleteProvider(index: number) {
    setForm((f) => {
      const removed = f.providers[index];
      const providers = f.providers.filter((_, i) => i !== index);
      const activeProvider = f.activeProvider === removed.id ? (providers[0]?.id ?? "") : f.activeProvider;
      // Same dangling-reference risk as the rename case in updateProvider: an agent whose
      // default model was this provider must fall back to "no default" instead of keeping a
      // providerId that no longer resolves to anything.
      const agents = f.agents.map((a) => (a.providerId === removed.id ? { ...a, providerId: "" } : a));
      // A combo loses it too; one left with nothing to try goes, with whatever pointed at it.
      const kept = f.combos.map((c) => ({ ...c, providers: c.providers.filter((id) => id !== removed.id) }));
      const emptied = kept.filter((c) => c.providers.length === 0).map((c) => c.id);
      // The policies it answered go with it, and no agent may delegate with it or them any more (P123).
      const cleaned = [removed.id, ...emptied].reduce<PolicyForm>((acc, id) => dropModel(acc, id), { agents, modelPolicies: f.modelPolicies });
      return {
        ...f,
        providers,
        activeProvider: emptied.includes(activeProvider) ? (providers[0]?.id ?? "") : activeProvider,
        agents: cleaned.agents.map((a) => (emptied.includes(a.providerId) ? { ...a, providerId: "" } : a)),
        modelPolicies: cleaned.modelPolicies,
        combos: kept.filter((c) => c.providers.length > 0),
      };
    });
  }

  function addCombo() {
    setForm((f) => {
      let n = f.combos.length + 1;
      while (f.combos.some((c) => c.id === `combo-${n}`) || f.providers.some((p) => p.id === `combo-${n}`)) n += 1;
      return { ...f, combos: [...f.combos, { id: `combo-${n}`, providers: [] }] };
    });
  }

  function updateCombo(index: number, next: Combo) {
    setForm((f) => {
      const prevId = f.combos[index]?.id;
      const combos = f.combos.map((c, i) => (i === index ? next : c));
      if (prevId === undefined || prevId === next.id) {
        return { ...f, combos };
      }
      // Same rename cascade as a provider's: the active model and every agent that named it.
      const activeProvider = f.activeProvider === prevId ? next.id : f.activeProvider;
      const agents = f.agents.map((a) => (a.providerId === prevId ? { ...a, providerId: next.id } : a));
      return renameModel({ ...f, combos, activeProvider, agents }, prevId, next.id);
    });
  }

  function deleteCombo(index: number) {
    setForm((f) => {
      const removed = f.combos[index];
      return dropModel(
        {
          ...f,
          combos: f.combos.filter((_, i) => i !== index),
          activeProvider: f.activeProvider === removed.id ? (f.providers[0]?.id ?? "") : f.activeProvider,
          agents: f.agents.map((a) => (a.providerId === removed.id ? { ...a, providerId: "" } : a)),
        },
        removed.id,
      );
    });
  }

  function addPolicy() {
    setForm((f) => {
      const taken = delegationCandidates(f.providers, f.combos, f.modelPolicies);
      return { ...f, modelPolicies: [...f.modelPolicies, { id: nextPolicyId(taken), model: f.providers[0]?.id ?? "", description: "" }] };
    });
  }

  function updatePolicy(index: number, next: ModelPolicy) {
    setForm((f) => {
      const prevId = f.modelPolicies[index]?.id;
      const updated = { ...f, modelPolicies: f.modelPolicies.map((p, i) => (i === index ? next : p)) };
      return prevId === undefined ? updated : renamePolicy(updated, prevId, next.id);
    });
  }

  function deletePolicy(index: number) {
    setForm((f) => dropPolicy(f, f.modelPolicies[index].id));
  }

  function addAgent() {
    setForm((f) => ({
      ...f,
      agents: [
        ...f.agents,
        { id: nextAgentId(f.agents), persona: "", providerId: "", canDelegateToAgents: false, canManageAgents: false, canMessageAgents: false, canManageTasks: false, canStartTasks: true, canCreateWorkers: true, canMessageUser: true, canChooseModels: true, allowedTools: null, autonomy: 4, approvalRequired: [] },
      ],
    }));
  }

  // An SSH host's "available to" list names agents by id, so renaming or deleting an agent must
  // carry through to it — same dangling-reference care as the provider rename/delete above.
  function updateAgent(index: number, next: AgentEntry) {
    setForm((f) => {
      const prevId = f.agents[index].id;
      const sshHosts =
        prevId === next.id ? f.sshHosts : f.sshHosts.map((h) => ({ ...h, agents: h.agents.map((a) => (a === prevId ? next.id : a)) }));
      // P120: whoever reported to a renamed agent follows its new name.
      const agents = renameInReports(
        f.agents.map((a, i) => (i === index ? next : a)),
        prevId,
        next.id,
      );
      // P121: the entry of an agent allowed to start messages follows its new name.
      return { ...f, agents, sshHosts, outreach: renameOutreach(f.outreach, prevId, next.id) };
    });
  }

  function deleteAgent(index: number) {
    setForm((f) => {
      const removed = f.agents[index].id;
      // A host restricted only to the deleted agent must not fall through to "empty list = every
      // agent" — that would silently widen access. Pruning it to nothing switches it off instead.
      const sshHosts = f.sshHosts.map((h) => {
        if (!h.agents.includes(removed)) return h;
        const agents = h.agents.filter((a) => a !== removed);
        return { ...h, agents, enabled: agents.length === 0 ? false : h.enabled };
      });
      // P120: the ones that reported to it report to its superior from now on.
      return { ...f, agents: removeFromOrg(f.agents, removed), sshHosts, outreach: dropOutreach(f.outreach, removed) };
    });
  }

  function addSshHost() {
    setForm((f) => ({
      ...f,
      sshHosts: [...f.sshHosts, { id: nextSshHostId(f.sshHosts), host: "", user: "", port: 22, identityFile: "", enabled: false, agents: [], requireApproval: false }],
    }));
  }

  function updateSshHost(index: number, next: SshHostEntry) {
    setForm((f) => ({ ...f, sshHosts: f.sshHosts.map((h, i) => (i === index ? next : h)) }));
  }

  function deleteSshHost(index: number) {
    setForm((f) => ({ ...f, sshHosts: f.sshHosts.filter((_, i) => i !== index) }));
  }

  function addMcpServer(server: McpServer) {
    setForm((f) => ({ ...f, mcpServers: [...f.mcpServers, server] }));
  }

  function addBlankMcpServer() {
    addMcpServer({ name: "", command: "", args: [], env: {} });
  }

  function updateMcpServer(index: number, next: McpServer) {
    setForm((f) => ({ ...f, mcpServers: f.mcpServers.map((s, i) => (i === index ? next : s)) }));
  }

  function deleteMcpServer(index: number) {
    setForm((f) => ({ ...f, mcpServers: f.mcpServers.filter((_, i) => i !== index) }));
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSavedAt(null);

    const namelessProvider = form.providers.find((p) => p.id.trim() === "");
    if (namelessProvider) {
      setError(t("st.err.providerName"));
      return;
    }
    const ids = form.providers.map((p) => p.id.trim());
    const duplicate = ids.find((id, i) => ids.indexOf(id) !== i);
    if (duplicate) {
      setError(t("st.err.providerDup", { name: duplicate }));
      return;
    }

    const namelessServer = form.mcpServers.find((s) => s.name.trim() === "");
    if (namelessServer) {
      setError(t("st.err.mcpName"));
      return;
    }
    const incompleteServer = form.mcpServers.find((s) => (isMcpServerHttp(s) ? s.url.trim() === "" : s.command.trim() === ""));
    if (incompleteServer) {
      setError(t(isMcpServerHttp(incompleteServer) ? "st.err.mcpUrl" : "st.err.mcpCommand", { name: incompleteServer.name }));
      return;
    }

    const namelessAgent = form.agents.find((a) => a.id.trim() === "");
    if (namelessAgent) {
      setError(t("st.err.agentName"));
      return;
    }
    const agentIds = form.agents.map((a) => a.id.trim());
    const duplicateAgent = agentIds.find((id, i) => agentIds.indexOf(id) !== i);
    if (duplicateAgent) {
      setError(t("st.err.agentDup", { name: duplicateAgent }));
      return;
    }

    const spendingError = validateSpending(form.limits, form.prices);
    if (spendingError) {
      setError(spendingError);
      return;
    }

    // Mirrors save_settings's own all-or-nothing check for git_sync — catches it before the IPC
    // round-trip.
    const gitSyncFilled = form.gitSync ? [form.gitSync.remoteUrl, form.gitSync.token].filter((s) => s.trim() !== "").length : 0;
    if (gitSyncFilled === 1) {
      setError(t("st.err.git"));
      return;
    }

    setIsSaving(true);
    try {
      await invoke("save_settings", {
        payload: {
          version: form.version,
          providers: form.providers,
          active_provider: form.activeProvider,
          combos: form.combos,
          model_policies: form.modelPolicies,
          outreach: form.outreach,
          tool_categories: form.toolCategories,
          persist_grants: form.persistGrants,
          grant_ttl_days: form.grantTtlDays,
          vault_path: form.vaultPath,
          generated_path: form.generatedPath,
          tavily_key: form.tavilyKey,
          whisper_key: form.whisperKey,
          enable_shell: form.enableShell,
          mcp_servers: form.mcpServers,
          agents: form.agents,
          ssh_hosts: form.sshHosts,
          git_sync: gitSyncFilled === 2 ? form.gitSync : null,
          // `null` means "no limits written" (the safety net applies), `[]` means "all off" — sent as is.
          limits: form.limits,
          prices: form.prices,
        },
      });
      const refreshed = await invoke<Settings>("get_settings");
      setForm(refreshed);
      setSavedAt(Date.now());
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSaving(false);
    }
  }

  if (isLoading) {
    return (
      <div className="settings-view">
        <p>{t("st.loading")}</p>
      </div>
    );
  }

  return (
    <div className="settings-view">
      <h2 className="settings-title">{t("st.title")}</h2>
      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("st.language.title")}</h3>
          <LanguageSwitch />
        </div>
        <p className="settings-hint">{t("st.language.hint")}</p>
      </section>
      <section className="settings-section">
        <div className="settings-section-header">
          <h3 className="settings-section-title">{t("st.theme.title")}</h3>
          <ThemeSwitch />
        </div>
        <p className="settings-hint">{t("st.theme.hint")}</p>
      </section>
      <form className="settings-form" onSubmit={handleSubmit}>
        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.providers.title")}</h3>
            <button type="button" className="settings-browse-btn" onClick={addProvider}>
              {t("st.providers.add")}
            </button>
          </div>
          {form.providers.length === 0 && <p className="settings-hint">{t("st.providers.none")}</p>}
          <div className="provider-list">
            {form.providers.map((p, i) => (
              <ProviderCard
                key={i}
                provider={p}
                isActive={form.activeProvider === p.id && p.id !== ""}
                defaultModel={form.defaultModels[p.kind]}
                onChange={(next) => updateProvider(i, next)}
                onDelete={() => deleteProvider(i)}
                onSetActive={() => setForm((f) => ({ ...f, activeProvider: p.id }))}
              />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.combos.title")}</h3>
            <button type="button" className="settings-browse-btn" onClick={addCombo}>
              {t("st.combos.add")}
            </button>
          </div>
          <p className="settings-hint">{t("st.combos.hint")}</p>
          {form.combos.length === 0 && <p className="settings-hint">{t("st.combos.none")}</p>}
          <div className="provider-list">
            {form.combos.map((c, i) => (
              <ComboCard
                key={i}
                combo={c}
                providers={form.providers}
                isActive={form.activeProvider === c.id && c.id !== ""}
                onChange={(next) => updateCombo(i, next)}
                onDelete={() => deleteCombo(i)}
                onSetActive={() => setForm((f) => ({ ...f, activeProvider: c.id }))}
              />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.policies.title")}</h3>
            <button type="button" className="settings-browse-btn" onClick={addPolicy} disabled={form.providers.length === 0 && form.combos.length === 0}>
              {t("st.policies.add")}
            </button>
          </div>
          <p className="settings-hint">{t("st.policies.hint")}</p>
          {form.modelPolicies.length === 0 && <p className="settings-hint">{t("st.policies.none")}</p>}
          <div className="provider-list">
            {form.modelPolicies.map((p, i) => (
              <PolicyCard
                key={i}
                policy={p}
                modelChoices={delegationCandidates(form.providers, form.combos, [])}
                onChange={(next) => updatePolicy(i, next)}
                onDelete={() => deletePolicy(i)}
              />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.agents.title")}</h3>
            <button type="button" className="settings-browse-btn" onClick={addAgent}>
              {t("st.agents.add")}
            </button>
          </div>
          <p className="settings-hint">{t("st.agents.hint")}</p>
          {form.agents.length === 0 && <p className="settings-hint">{t("st.agents.none")}</p>}
          <div className="provider-list">
            {form.agents.map((a, i) => (
              <AgentCard key={i} agent={a} outreach={form.outreach} onOutreach={(next) => setForm((f) => ({ ...f, outreach: next }))} allAgents={form.agents} providers={form.providers} combos={form.combos} modelChoices={delegationCandidates(form.providers, form.combos, form.modelPolicies)} toolNames={toolNames} people={people} onChange={(next) => updateAgent(i, next)} onDelete={() => deleteAgent(i)} />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.risk.title")}</h3>
            <button
              type="button"
              className="settings-browse-btn"
              onClick={() => setForm((f) => ({ ...f, toolCategories: [...f.toolCategories, { tool: "", category: APPROVAL_CATEGORIES[0].id }] }))}
            >
              {t("st.risk.add")}
            </button>
          </div>
          <p className="settings-hint">{t("st.risk.hint")}</p>
          {form.toolCategories.length === 0 && <p className="settings-hint">{t("st.risk.none")}</p>}
          <div className="provider-list">
            {form.toolCategories.map((entry, i) => (
              <ToolCategoryRow
                key={i}
                entry={entry}
                onChange={(next) => setForm((f) => ({ ...f, toolCategories: f.toolCategories.map((c, j) => (j === i ? next : c)) }))}
                onDelete={() => setForm((f) => ({ ...f, toolCategories: f.toolCategories.filter((_, j) => j !== i) }))}
              />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <h3 className="settings-section-title">{t("st.grants.title")}</h3>
          <p className="settings-hint">{t("st.grants.hint")}</p>
          <label className="settings-field settings-checkbox-field">
            <span className="settings-checkbox-row">
              <input type="checkbox" checked={form.persistGrants} onChange={(e) => setForm((f) => ({ ...f, persistGrants: e.currentTarget.checked }))} />
              <span className="settings-label">{t("st.grants.keep")}</span>
            </span>
          </label>
          <label className="settings-field">
            <span className="settings-label">{t("st.grants.days")}</span>
            <input
              className="settings-input"
              type="number"
              min={1}
              value={form.grantTtlDays || ""}
              disabled={!form.persistGrants}
              onChange={(e) => setForm((f) => ({ ...f, grantTtlDays: Math.floor(Number(e.currentTarget.value)) }))}
            />
          </label>
        </section>

        <SpendingSection
          limits={form.limits}
          defaultLimits={form.defaultLimits}
          disabledByEnv={form.limitsDisabledByEnv}
          prices={form.prices}
          agents={form.agents}
          modelSuggestions={[...new Set([...form.providers.map((p) => p.model), ...Object.values(form.defaultModels)].filter((m) => m.trim() !== ""))]}
          onLimitsChange={(limits) => setForm((f) => ({ ...f, limits }))}
          onPricesChange={(prices) => setForm((f) => ({ ...f, prices }))}
        />

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.sshServers.title")}</h3>
            <button type="button" className="settings-browse-btn" onClick={addSshHost}>
              {t("st.sshServers.add")}
            </button>
          </div>
          <p className="settings-hint">{t("st.sshServers.hint")}</p>
          {form.sshHosts.length === 0 && <p className="settings-hint">{t("st.sshServers.none")}</p>}
          <div className="provider-list">
            {form.sshHosts.map((h, i) => (
              <SshHostCard key={i} host={h} agents={form.agents} onChange={(next) => updateSshHost(i, next)} onDelete={() => deleteSshHost(i)} />
            ))}
          </div>
          <SshAuditLog />
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.mcpServers.title")}</h3>
          </div>
          <p className="settings-hint">{t("st.mcpServers.hint")}</p>
          <div className="provider-list">
            {MCP_CATALOG.map((entry) => {
              const added = catalogState(entry, form.mcpServers) === "added";
              return (
                <div className="provider-card skill-card" key={entry.id}>
                  <div className="skill-card-header">
                    <span className="skill-card-name">{t(entry.label)}</span>
                    {added ? (
                      <span className="settings-hint">{t("st.mcpCat.added")}</span>
                    ) : (
                      <button type="button" className="settings-browse-btn" onClick={() => addMcpServer(catalogServer(entry, form.mcpServers))}>
                        {t("st.mcpCat.add")}
                      </button>
                    )}
                  </div>
                  <p className="skill-card-description">{t(entry.description)}</p>
                  <p className="settings-hint">{t("st.mcpCat.needs", { what: t(MCP_NEEDS_TEXT[entry.needs]) })}</p>
                </div>
              );
            })}
          </div>
          <div className="mcp-quick-add-row">
            <button type="button" className="settings-browse-btn" onClick={addBlankMcpServer}>
              {t("st.mcpServers.custom")}
            </button>
          </div>
          {form.mcpServers.length === 0 && <p className="settings-hint">{t("st.mcpServers.none")}</p>}
          <div className="provider-list">
            {form.mcpServers.map((s, i) => (
              <McpServerCard key={i} server={s} onChange={(next) => updateMcpServer(i, next)} onDelete={() => deleteMcpServer(i)} />
            ))}
          </div>
        </section>

        <section className="settings-section">
          <div className="settings-section-header">
            <h3 className="settings-section-title">{t("st.gitSync.title")}</h3>
          </div>
          <p className="settings-hint">{t("st.gitSync.hint")}</p>
          <GitSyncForm value={form.gitSync} onChange={(gitSync) => setForm((f) => ({ ...f, gitSync }))} />
        </section>

        <label className="settings-field">
          <span className="settings-label">{t("st.vaultPath")}</span>
          <div className="settings-key-field">
            <input
              className="settings-input"
              type="text"
              placeholder={t("st.vaultPathPlaceholder")}
              value={form.vaultPath}
              onChange={(e) => setForm((f) => ({ ...f, vaultPath: e.currentTarget.value }))}
            />
            <button type="button" className="settings-browse-btn" onClick={handleBrowseVaultPath}>
              {t("st.browse")}
            </button>
          </div>
        </label>

        <label className="settings-field">
          <span className="settings-label">{t("st.generatedPath")}</span>
          <p className="settings-hint">{t("st.generatedPathHint")}</p>
          <div className="settings-key-field">
            <input
              className="settings-input"
              type="text"
              placeholder={t("st.generatedPathPlaceholder")}
              value={form.generatedPath}
              onChange={(e) => setForm((f) => ({ ...f, generatedPath: e.currentTarget.value }))}
            />
            <button type="button" className="settings-browse-btn" onClick={handleBrowseGeneratedPath}>
              {t("st.browse")}
            </button>
          </div>
        </label>

        <ApiKeyField
          label={t("st.tavily")}
          value={form.tavilyKey}
          onChange={(v) => setForm((f) => ({ ...f, tavilyKey: v }))}
        />

        <ApiKeyField
          label={t("st.whisper")}
          value={form.whisperKey}
          onChange={(v) => setForm((f) => ({ ...f, whisperKey: v }))}
        />

        <label className="settings-field settings-checkbox-field">
          <span className="settings-checkbox-row">
            <input
              type="checkbox"
              checked={form.enableShell}
              onChange={(e) => {
                const enableShell = e.currentTarget.checked;
                setForm((f) => ({ ...f, enableShell }));
              }}
            />
            <span className="settings-label">{t("st.shell")}</span>
          </span>
          <span className="settings-hint">{t("st.shellHint")}</span>
        </label>

        {error && <p className="settings-error-banner">{error}</p>}
        {savedAt && !error && <p className="settings-success-banner">{t("st.saved")}</p>}

        <button type="submit" className="settings-save-btn" disabled={isSaving}>
          {isSaving ? t("common.saving") : t("st.save")}
        </button>
      </form>

      {/* P118 — saved on its own, outside the form above. */}
      <BotsSection />

      {/* P12 — saved on its own, outside the form above. */}
      <ApiKeysSection />
    </div>
  );
}

export default SettingsView;
