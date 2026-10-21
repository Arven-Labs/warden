import { useCallback, useEffect, useMemo, useState } from "react";
import { SettingsError, UserError, type LoadedSettings, type ServerConnection } from "../hub/connection";
import { APPROVAL_CATEGORIES } from "../hub/approvalCategories";
import { descendantsOf, removeFromOrg, renameInReports } from "../hub/org";
import { t } from "../i18n";
import ApiKeysSection from "./ApiKeysSection";
import AdvancedSection from "./AdvancedSection";
import MachineSection from "./MachineSection";
import ProviderKeyTest from "./ProviderKeyTest";
import { advancedError, machineError, machineSummary, toAdvanced, toAdvancedDraft, toMachineDraft, toMachineEdit, type AdvancedDraft, type MachineDraft } from "./machineDraft";
import { Field, KEEP, keyed, SecretField, Section, strip, type Keyed, type SecretDraft } from "./settingsParts";
import type {
  AdvancedSettings,
  AgentSettings,
  BotPairing,
  BotPairingsView,
  BotsSettings,
  Combo,
  HubSettings,
  HubSettingsUpdate,
  LimitScope,
  LimitSettings,
  MachineEdit,
  ModelPolicy,
  OutreachEntry,
  PriceSettings,
  ProviderEdit,
  ProviderKind,
  ToolCategoryEntry,
  UserInfo,
} from "../hub/messages";
import { delegationCandidates, delegationSummary, dropModel, dropPolicy, nextPolicyId, renameModel, renamePolicy } from "../hub/modelPolicies";
import { dropOutreach, OUTREACH_CHANNELS, outreachForward, outreachOn, renameOutreach, setForward, setOutreach } from "../hub/outreach";

// The hub's settings (P78): providers, agents, the Tavily/Whisper keys, spending limits, prices and
// the git remote the vault syncs to (P61; the Sync tab runs it).
// Everything is edited as one draft and saved at once; the hub asks for the pairing key again on
// every save, checks the file wasn't changed meanwhile, and restarts its orchestrator with the new
// settings (or keeps the old ones if it can't start with them). API keys never come back from the
// hub: a key field shows whether one is saved and can only be replaced or removed.

/** Built on each read, so the labels follow the language the person has now. */
const kinds = (): { value: ProviderKind; label: string }[] => [
  { value: "gemini", label: "Gemini" },
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "openai_compatible", label: t("settings.kind.openai_compatible") },
  { value: "node", label: t("settings.kind.node") },
  { value: "cloud", label: t("settings.kind.cloud") },
];

const scopes = (): { value: LimitScope; label: string; target: string }[] => [
  { value: "global", label: t("settings.scope.global"), target: "" },
  { value: "agent", label: t("settings.scope.agent"), target: t("settings.scopeTarget.agent") },
  { value: "channel", label: t("settings.scope.channel"), target: t("settings.scopeTarget.channel") },
  { value: "user", label: t("settings.scope.user"), target: t("settings.scopeTarget.user") },
  { value: "person", label: t("settings.scope.person"), target: t("settings.scopeTarget.person") },
];

type ProviderDraft = Keyed<{ originalId?: string; id: string; kind: ProviderKind; baseUrl: string; model: string; apiKey: SecretDraft; node: string }>;

type LimitsMode = "default" | "custom" | "off";

interface Draft {
  providers: ProviderDraft[];
  activeProvider: string;
  combos: Keyed<Combo>[];
  /** P123 — named model policies. */
  modelPolicies: Keyed<ModelPolicy>[];
  /** P121 — the agents that can start messages, by agent name (follows renaming and removal). */
  outreach: OutreachEntry[];
  /** P122 — the map from tool to risk category. */
  toolCategories: ToolCategoryEntry[];
  /** P122 — the "always" kept in a file. An older hub does not send the two fields: the section disappears and none of this is sent. */
  grantsSupported: boolean;
  persistGrants: boolean;
  grantTtlDays: string;
  agents: Keyed<AgentSettings>[];
  tavilyKey: SecretDraft;
  whisperKey: SecretDraft;
  limitsMode: LimitsMode;
  limits: Keyed<LimitSettings>[];
  prices: Keyed<PriceSettings>[];
  gitRemoteUrl: string;
  /** As loaded: an untouched git section isn't sent, so a remote set by hand in the file (a local
   * path, which the web can't save) never blocks saving the rest. */
  gitRemoteLoaded: string;
  gitToken: SecretDraft;
  /** P118 — the lists are edited as text, one entry per line. */
  botsLearningEnabled: boolean;
  botsLearningProvider: string;
  botsMaxPerDay: string;
  botsLearningChats: string;
  botsTelegramUsers: string;
  botsWhatsappChats: string;
  botsTelegramPairing: boolean;
  botsWhatsappPairing: boolean;
  /** P119 — the Telegram bot's token, a secret like the keys above. */
  telegramToken: SecretDraft;
  /** P119 — delegation ceilings and TruthID. Sent only when they changed from `advancedBase` (as loaded). */
  advanced: AdvancedDraft;
  advancedBase: AdvancedSettings;
  /** P119 — what reaches the hub's machine. Sent only when it changed from `machineBase`, so a hub that
   * doesn't allow editing it (or a connection that isn't encrypted) never receives it by accident. */
  machine: MachineDraft;
  machineBase: MachineEdit;
}

function toDraft(s: HubSettings): Draft {
  return {
    providers: s.providers.map((p) => keyed({ originalId: p.id, id: p.id, kind: p.kind, baseUrl: p.baseUrl, model: p.model, apiKey: { saved: p.apiKey, edit: KEEP }, node: p.node ?? "" })),
    activeProvider: s.activeProvider,
    combos: (s.combos ?? []).map((c) => keyed({ ...c })),
    modelPolicies: (s.modelPolicies ?? []).map((p) => keyed({ ...p })),
    outreach: (s.outreach ?? []).map((o) => ({ ...o, forward: [...o.forward] })),
    toolCategories: (s.toolCategories ?? []).map((c) => ({ ...c })),
    grantsSupported: s.grantTtlDays !== undefined,
    persistGrants: s.persistGrants ?? true,
    grantTtlDays: s.grantTtlDays === undefined ? "" : String(s.grantTtlDays),
    agents: s.agents.map((a) => keyed({ ...a, originalId: a.id })),
    tavilyKey: { saved: s.tavilyKey, edit: KEEP },
    whisperKey: { saved: s.whisperKey, edit: KEEP },
    limitsMode: s.limits === null ? "default" : s.limits.length === 0 ? "off" : "custom",
    limits: (s.limits ?? []).map(keyed),
    prices: s.prices.map(keyed),
    gitRemoteUrl: s.gitSync.remoteUrl,
    gitRemoteLoaded: s.gitSync.remoteUrl,
    gitToken: { saved: s.gitSync.token, edit: KEEP },
    botsLearningEnabled: s.bots.learningEnabled,
    botsLearningProvider: s.bots.learningProvider,
    botsMaxPerDay: String(s.bots.learningMaxPerDay),
    botsLearningChats: s.bots.learningBotChats.join("\n"),
    botsTelegramUsers: s.bots.telegramAllowedUsers.join("\n"),
    botsWhatsappChats: s.bots.whatsappAllowedChats.join("\n"),
    botsTelegramPairing: s.bots.telegramPairing,
    botsWhatsappPairing: s.bots.whatsappPairing,
    telegramToken: { saved: s.telegramToken, edit: KEEP },
    advanced: toAdvancedDraft(s.advanced),
    advancedBase: s.advanced,
    machine: toMachineDraft(s.machine),
    machineBase: toMachineEdit(toMachineDraft(s.machine)),
  };
}

/** One entry per line (a comma works too), blanks dropped. */
function lines(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((l) => l.trim())
    .filter((l) => l !== "");
}

function toBots(d: Draft): BotsSettings {
  return {
    learningEnabled: d.botsLearningEnabled,
    learningProvider: d.botsLearningProvider,
    learningMaxPerDay: Number(d.botsMaxPerDay),
    learningBotChats: lines(d.botsLearningChats),
    telegramAllowedUsers: lines(d.botsTelegramUsers).map(Number),
    whatsappAllowedChats: lines(d.botsWhatsappChats),
    telegramPairing: d.botsTelegramPairing,
    whatsappPairing: d.botsWhatsappPairing,
  };
}

/** What stops the bots block from being saved, in the words the screen shows; `null` when it's fine. */
function botsError(d: Draft): string | null {
  const max = Number(d.botsMaxPerDay);
  if (!Number.isInteger(max) || max < 1) return t("settings.botsMax");
  const bad = lines(d.botsTelegramUsers).find((l) => !/^[1-9][0-9]*$/.test(l));
  if (bad) return t("settings.botsBadTelegram", { bad });
  const chat = lines(d.botsLearningChats).find((l) => !/^(telegram|whatsapp):\S+$/.test(l));
  if (chat) return t("settings.botsBadChat", { chat });
  const wa = lines(d.botsWhatsappChats).find((l) => /\s/.test(l));
  if (wa) return t("settings.botsSpaces", { wa });
  return null;
}

/** A provider as a save sends it, which is also how "Test key" (P10) sends the one being tested. */
function providerEdit(p: ProviderDraft): ProviderEdit {
  return { originalId: p.originalId, id: p.id, kind: p.kind, baseUrl: p.baseUrl, model: p.model, apiKey: p.apiKey.edit, node: p.kind === "node" ? p.node : "" };
}

function toUpdate(d: Draft): HubSettingsUpdate {
  return {
    providers: d.providers.map(providerEdit),
    activeProvider: d.activeProvider,
    combos: d.combos.map(strip),
    modelPolicies: d.modelPolicies.map(strip),
    outreach: d.outreach,
    toolCategories: d.toolCategories,
    ...(d.grantsSupported && { persistGrants: d.persistGrants, grantTtlDays: Math.floor(Number(d.grantTtlDays)) }),
    agents: d.agents.map(strip),
    tavilyKey: d.tavilyKey.edit,
    whisperKey: d.whisperKey.edit,
    limits: d.limitsMode === "default" ? null : d.limitsMode === "off" ? [] : d.limits.map(strip),
    prices: d.prices.map(strip),
    ...((d.gitRemoteUrl !== d.gitRemoteLoaded || d.gitToken.edit.action !== "keep") && {
      gitSync: { remoteUrl: d.gitRemoteUrl, token: d.gitRemoteUrl.trim() === "" ? { action: "clear" } : d.gitToken.edit },
    }),
    bots: toBots(d),
    telegramToken: d.telegramToken.edit,
    ...(machineChanged(d) && { machine: toMachineEdit(d.machine) }),
    ...(advancedChanged(d) && { advanced: toAdvanced(d.advanced) }),
  };
}

function machineChanged(d: Draft): boolean {
  return JSON.stringify(toMachineEdit(d.machine)) !== JSON.stringify(d.machineBase);
}

function advancedChanged(d: Draft): boolean {
  return JSON.stringify(toAdvanced(d.advanced)) !== JSON.stringify(d.advancedBase);
}

/** `list` with the item at `index` moved one place up (`-1`) or down (`1`). */
function moved(list: string[], index: number, delta: number): string[] {
  const next = [...list];
  const [item] = next.splice(index, 1);
  next.splice(index + delta, 0, item);
  return next;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function numberOrNull(value: string): number | null {
  return value.trim() === "" ? null : Number(value);
}

/** P117 — people who wrote to a bot and were given a code. The owner approves (they join the bot's list) or
 * denies each one, with the pairing key. Not part of the draft: it acts on its own, so it's off while the
 * form has unsaved edits (an approval reloads the settings, and the lists in them change). */
function BotPairings({ conn, disabled, onResolved }: { conn: ServerConnection | null; disabled: boolean; onResolved: () => void }) {
  const [view, setView] = useState<BotPairingsView>({ pairings: [], members: [] });
  const { pairings, members } = view;
  /** `member` empty: the chat is answered as the owner. */
  const [ask, setAsk] = useState<{ code: string; approve: boolean; member: string } | null>(null);
  /** P117 — who each waiting request is approved as speaking as, by code. Missing or "" is the owner. */
  const [speakAs, setSpeakAs] = useState<Record<string, string>>({});
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listBotPairings().then(setView, () => setView({ pairings: [], members: [] }));
  }, [conn]);
  useEffect(refresh, [refresh]);

  const nameOf = (id: string) => members.find((m) => m.id === id)?.name ?? id;

  async function resolve() {
    if (!conn || !ask) return;
    setBusy(true);
    setError(null);
    try {
      setView(await conn.resolveBotPairing(key, ask.code, ask.approve, ask.member || undefined));
      const approved = ask.approve;
      setAsk(null);
      setKey("");
      if (approved) onResolved();
    } catch (err) {
      setError(err instanceof UserError && err.authRejected ? t("pairingKey.wrong") : message(err));
    } finally {
      setBusy(false);
    }
  }

  const minutes = (p: BotPairing) => Math.max(0, Math.ceil((p.expiresAt * 1000 - Date.now()) / 60000));

  return (
    <div className="settings-field">
      <strong>{t("pairings.title")}</strong>
      {pairings.length === 0 ? (
        <p className="skills-hint">{t("pairings.none")}</p>
      ) : (
        <ul className="skills-list">
          {pairings.map((p) => (
            <li key={`${p.channel}:${p.sender}`} className="skills-item settings-card">
              <p>
                <code>{p.code}</code> {p.channel === "telegram" ? t("pairings.telegram") : t("pairings.whatsapp")} {p.sender}
                {p.label && ` (${p.label})`}
                {t("pairings.expires", { minutes: minutes(p) })}
              </p>
              <Field label={t("pairings.speakAs")} hint={t("pairings.speakAsHint")}>
                <select value={speakAs[p.code] ?? ""} onChange={(e) => setSpeakAs({ ...speakAs, [p.code]: e.target.value })}>
                  <option value="">{t("pairings.asOwner")}</option>
                  {members.map((m) => (
                    <option key={m.id} value={m.id} disabled={!m.linked}>
                      {m.name} ({m.id}){m.linked ? "" : t("pairings.linkFirst")}
                    </option>
                  ))}
                </select>
              </Field>
              <div className="skills-actions">
                <button type="button" className="primary-button" disabled={disabled || !conn} onClick={() => setAsk({ code: p.code, approve: true, member: speakAs[p.code] ?? "" })}>
                  {t("pairings.approve")}
                </button>
                <button type="button" className="link-button skills-danger" disabled={disabled || !conn} onClick={() => setAsk({ code: p.code, approve: false, member: "" })}>
                  {t("pairings.refuse")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {disabled && pairings.length > 0 && <p className="skills-hint">{t("pairings.saveFirst")}</p>}
      {ask && (
        <form
          className="settings-confirm"
          onSubmit={(e) => {
            e.preventDefault();
            void resolve();
          }}
        >
          <Field label={`${t(ask.approve ? "pairings.keyLabelApprove" : "pairings.keyLabelRefuse", { code: ask.code })}${ask.approve && ask.member ? t("pairings.asMember", { name: nameOf(ask.member) }) : ""}`}>
            <input type="password" autoComplete="current-password" autoFocus value={key} onChange={(e) => setKey(e.target.value)} />
          </Field>
          {error && <p className="error-banner">{error}</p>}
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={busy || key.trim() === ""}>
              {t("pairings.confirm")}
            </button>
            <button
              type="button"
              className="link-button"
              disabled={busy}
              onClick={() => {
                setAsk(null);
                setKey("");
                setError(null);
              }}
            >
              {t("common.cancel")}
            </button>
          </div>
        </form>
      )}
      <button type="button" className="link-button" onClick={refresh}>
        {t("pairings.refresh")}
      </button>
    </div>
  );
}

export default function SettingsView({ conn }: { conn: ServerConnection | null }) {
  const [loaded, setLoaded] = useState<LoadedSettings | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<{ text: string; conflict: boolean } | null>(null);
  const [asking, setAsking] = useState(false);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  /** P119 — a save that changes what reaches the hub's machine is read back to the person first. */
  const [confirmingMachine, setConfirmingMachine] = useState(false);
  const [machineAck, setMachineAck] = useState(false);
  /** P84 — the workspace's members, for sharing agents with them. */
  const [people, setPeople] = useState<UserInfo[]>([]);

  const load = useCallback(() => {
    if (!conn) return;
    conn
      .listUsers()
      .then(({ users }) => setPeople(users))
      .catch(() => setPeople([]));
    conn.requestSettings().then(
      (result) => {
        setLoaded(result);
        setDraft(toDraft(result.settings));
        setLoadError(null);
        setSaveError(null);
      },
      (err) => setLoadError(t("settings.loadError", { error: message(err) })),
    );
  }, [conn]);

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(load, [load]);

  const dirty = useMemo(() => {
    if (!loaded || !draft) return false;
    return JSON.stringify(toUpdate(draft)) !== JSON.stringify(toUpdate(toDraft(loaded.settings)));
  }, [loaded, draft]);

  if (!loaded || !draft) {
    return <div className="usage-view">{loadError ? <p className="error-banner">{loadError}</p> : <p className="skills-hint">{t("common.loading")}</p>}</div>;
  }

  const { settings, secretsWritable } = loaded;
  const botsProblem = botsError(draft);
  // The machine and advanced blocks are only questioned when they changed: a file edited by hand into
  // something the screen wouldn't write must not stop the rest of it from saving.
  const machineIsChanged = machineChanged(draft);
  const machineProblem = machineIsChanged ? machineError(draft.machine, draft.machineBase) : null;
  const advancedProblem = advancedChanged(draft) ? advancedError(draft.advanced, draft.advancedBase) : null;
  const agentIds = draft.agents.map((a) => a.id.trim()).filter((id) => id !== "");
  const modelIds = [...draft.providers.map((p) => p.id), ...draft.combos.map((c) => c.id)].filter((id) => id.trim() !== "");
  const update = (change: (d: Draft) => Draft) => {
    setDraft((d) => (d ? change(d) : d));
    setSavedAt(null);
  };

  function patchProvider(key: number, patch: Partial<ProviderDraft>) {
    update((d) => {
      const before = d.providers.find((p) => p.key === key);
      const providers = d.providers.map((p) => (p.key === key ? { ...p, ...patch } : p));
      // A renamed provider stays the active one and every agent's default (same cascade as the desktop).
      if (before && patch.id !== undefined && patch.id !== before.id) {
        return renameModel(
          {
            ...d,
            providers,
            activeProvider: d.activeProvider === before.id ? patch.id : d.activeProvider,
            agents: d.agents.map((a) => (a.providerId === before.id ? { ...a, providerId: patch.id! } : a)),
            combos: d.combos.map((c) => ({ ...c, providers: c.providers.map((id) => (id === before.id ? patch.id! : id)) })),
          },
          before.id,
          patch.id,
        );
      }
      return { ...d, providers };
    });
  }

  function removeProvider(key: number) {
    update((d) => {
      const removed = d.providers.find((p) => p.key === key);
      const next = {
        ...d,
        providers: d.providers.filter((p) => p.key !== key),
        activeProvider: d.activeProvider === removed?.id ? "" : d.activeProvider,
        agents: d.agents.map((a) => (a.providerId === removed?.id ? { ...a, providerId: "" } : a)),
        // A combo loses it too (the hub refuses an empty combo, so the screen says so before saving).
        combos: d.combos.map((c) => ({ ...c, providers: c.providers.filter((id) => id !== removed?.id) })),
      };
      // P123: the policies it answered go with it, and no agent may delegate with it or them any more.
      return removed ? dropModel(next, removed.id) : next;
    });
  }

  function patchCombo(key: number, patch: Partial<Combo>) {
    update((d) => {
      const before = d.combos.find((c) => c.key === key);
      const combos = d.combos.map((c) => (c.key === key ? { ...c, ...patch } : c));
      // A renamed combo stays the active model and every agent's default, like a provider.
      if (before && patch.id !== undefined && patch.id !== before.id) {
        return renameModel(
          {
            ...d,
            combos,
            activeProvider: d.activeProvider === before.id ? patch.id : d.activeProvider,
            agents: d.agents.map((a) => (a.providerId === before.id ? { ...a, providerId: patch.id! } : a)),
          },
          before.id,
          patch.id,
        );
      }
      return { ...d, combos };
    });
  }

  function removeCombo(key: number) {
    update((d) => {
      const removed = d.combos.find((c) => c.key === key);
      const next = {
        ...d,
        combos: d.combos.filter((c) => c.key !== key),
        activeProvider: d.activeProvider === removed?.id ? "" : d.activeProvider,
        agents: d.agents.map((a) => (a.providerId === removed?.id ? { ...a, providerId: "" } : a)),
      };
      return removed ? dropModel(next, removed.id) : next;
    });
  }

  function patchPolicy(key: number, patch: Partial<ModelPolicy>) {
    update((d) => {
      const before = d.modelPolicies.find((p) => p.key === key);
      const next = { ...d, modelPolicies: d.modelPolicies.map((p) => (p.key === key ? { ...p, ...patch } : p)) };
      return before && patch.id !== undefined ? renamePolicy(next, before.id, patch.id) : next;
    });
  }

  function removePolicy(key: number) {
    update((d) => {
      const removed = d.modelPolicies.find((p) => p.key === key);
      return removed ? dropPolicy(d, removed.id) : d;
    });
  }

  function patchAgent(key: number, patch: Partial<AgentSettings>) {
    update((d) => {
      const before = d.agents.find((a) => a.key === key);
      const agents = d.agents.map((a) => (a.key === key ? { ...a, ...patch } : a));
      // P120: quem reportava a um agente renomeado acompanha o nome novo.
      const renamed = before && patch.id !== undefined;
      return {
        ...d,
        agents: renamed ? renameInReports(agents, before.id, patch.id!) : agents,
        // P121: the entry of who can start messages also follows the new name.
        outreach: renamed ? renameOutreach(d.outreach, before.id, patch.id!) : d.outreach,
      };
    });
  }

  function patchLimit(key: number, patch: Partial<LimitSettings>) {
    update((d) => ({ ...d, limits: d.limits.map((l) => (l.key === key ? { ...l, ...patch } : l)) }));
  }

  function patchPrice(key: number, patch: Partial<PriceSettings>) {
    update((d) => ({ ...d, prices: d.prices.map((p) => (p.key === key ? { ...p, ...patch } : p)) }));
  }

  function setLimitsMode(mode: LimitsMode) {
    update((d) => ({
      ...d,
      limitsMode: mode,
      // "Customize" starts from the built-in limits, so the numbers live in one place (the hub).
      limits: mode === "custom" && d.limits.length === 0 ? settings.defaultLimits.map(keyed) : d.limits,
    }));
  }

  async function save() {
    if (!conn || !draft || !loaded) return;
    setSaving(true);
    setKeyError(null);
    setSaveError(null);
    try {
      const result = await conn.saveSettings(pairingKey, loaded.version, toUpdate(draft));
      setLoaded({ ...loaded, settings: result.settings, version: result.version });
      setDraft(toDraft(result.settings));
      setAsking(false);
      setConfirmingMachine(false);
      setMachineAck(false);
      setPairingKey("");
      setSavedAt(Date.now());
    } catch (err) {
      if (err instanceof SettingsError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        setAsking(false);
        setPairingKey("");
        setSaveError({ text: message(err), conflict: err instanceof SettingsError && err.conflict });
      }
    } finally {
      setSaving(false);
    }
  }

  const providerIds = draft.providers.map((p) => p.id.trim()).filter((id) => id !== "");
  const comboIds = draft.combos.map((c) => c.id.trim()).filter((id) => id !== "");

  return (
    <div className="usage-view settings-view">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("settings.intro")}</span>
        <button type="button" className="link-button" onClick={load} disabled={!conn || saving}>
          {t("devices.reload")}
        </button>
      </div>

      {settings.notes.map((note) => (
        <p key={note} className="banner settings-note">
          {note}
        </p>
      ))}
      {!secretsWritable && <p className="banner settings-note">{t("settings.insecure")}</p>}

      <Section title={t("settings.providers.title")} hint={t("settings.providers.hint")}
        action={
          <button
            type="button"
            className="link-button"
            onClick={() =>
              update((d) => ({
                ...d,
                providers: [...d.providers, keyed({ id: "", kind: "gemini" as ProviderKind, baseUrl: "", model: "", apiKey: { saved: { set: false }, edit: KEEP }, node: "" })],
              }))
            }
          >
            {t("settings.providers.add")}
          </button>
        }
      >
        {draft.providers.length === 0 && <p className="skills-hint">{t("settings.providers.none")}</p>}
        <ul className="skills-list">
          {draft.providers.map((p) => (
            <li key={p.key} className="skills-item settings-card">
              <div className="skills-item-header">
                <label className="settings-radio">
                  <input
                    type="radio"
                    name="active-provider"
                    checked={p.id.trim() !== "" && draft.activeProvider === p.id}
                    disabled={p.id.trim() === ""}
                    onChange={() => update((d) => ({ ...d, activeProvider: p.id }))}
                  />
                  {draft.activeProvider === p.id && p.id.trim() !== "" ? t("settings.active") : t("settings.useThis")}
                </label>
                <button type="button" className="link-button skills-danger" onClick={() => removeProvider(p.key)}>
                  {t("settings.remove")}
                </button>
              </div>
              <div className="settings-grid">
                <Field label={t("common.name")}>
                  <input value={p.id} onChange={(e) => patchProvider(p.key, { id: e.target.value })} />
                </Field>
                <Field label={t("settings.providers.type")}>
                  <select value={p.kind} onChange={(e) => patchProvider(p.key, { kind: e.target.value as ProviderKind })}>
                    {kinds().map((k) => (
                      <option key={k.value} value={k.value}>
                        {k.label}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label={p.kind === "node" ? t("settings.providers.nodeProvider") : t("settings.providers.model")} hint={p.kind === "node" ? t("settings.providers.nodeHint") : undefined}>
                  <input
                    value={p.model}
                    placeholder={p.kind === "node" ? "ollama" : (settings.defaultModels[p.kind] ?? t("settings.providers.required"))}
                    onChange={(e) => patchProvider(p.key, { model: e.target.value })}
                  />
                </Field>
                {p.kind === "cloud" && (
                  <Field label={t("settings.providers.baseUrl")} hint={t("settings.providers.cloudHint")}>
                    <input value={p.baseUrl} onChange={(e) => patchProvider(p.key, { baseUrl: e.target.value })} />
                  </Field>
                )}
                {p.kind === "node" && (
                  <Field label={t("settings.providers.nodeId")} hint={t("settings.providers.nodeIdHint")}>
                    <input value={p.node} placeholder="node-home-pc-1a2b3c4d" onChange={(e) => patchProvider(p.key, { node: e.target.value })} />
                  </Field>
                )}
                {p.kind === "openai_compatible" && (
                  <Field label={t("settings.providers.baseUrl")} hint={t("settings.providers.compatHint")}>
                    <input value={p.baseUrl} onChange={(e) => patchProvider(p.key, { baseUrl: e.target.value })} />
                  </Field>
                )}
                {p.kind !== "node" && (
                  <SecretField
                    label={t("settings.providers.apiKey")}
                    value={p.apiKey}
                    writable={secretsWritable}
                    onChange={(edit) => patchProvider(p.key, { apiKey: { ...p.apiKey, edit } })}
                  />
                )}
                {p.kind !== "node" && <ProviderKeyTest conn={conn} provider={providerEdit(p)} />}
              </div>
            </li>
          ))}
        </ul>
      </Section>

      <Section
        title={t("settings.combos.title")}
        hint={t("settings.combos.hint")}
        action={
          <button type="button" className="link-button" onClick={() => update((d) => ({ ...d, combos: [...d.combos, keyed({ id: "", providers: [] })] }))}>
            {t("settings.combos.add")}
          </button>
        }
      >
        {draft.combos.length === 0 && <p className="skills-hint">{t("settings.combos.none")}</p>}
        <ul className="skills-list">
          {draft.combos.map((c) => (
            <li key={c.key} className="skills-item settings-card">
              <div className="skills-item-header">
                <label className="settings-radio">
                  <input
                    type="radio"
                    name="active-provider"
                    checked={c.id.trim() !== "" && draft.activeProvider === c.id}
                    disabled={c.id.trim() === ""}
                    onChange={() => update((d) => ({ ...d, activeProvider: c.id }))}
                  />
                  {draft.activeProvider === c.id && c.id.trim() !== "" ? t("settings.active") : t("settings.useThis")}
                </label>
                <button type="button" className="link-button skills-danger" onClick={() => removeCombo(c.key)}>
                  {t("settings.remove")}
                </button>
              </div>
              <Field label={t("common.name")}>
                <input value={c.id} onChange={(e) => patchCombo(c.key, { id: e.target.value })} />
              </Field>
              {c.providers.length === 0 && <p className="error-banner">{t("settings.combos.needsOne")}</p>}
              {c.providers.length > 0 && (
                <ol className="fallback-list">
                  {c.providers.map((id, index) => (
                    <li key={id} className="fallback-row">
                      <span className="fallback-name">{id}</span>
                      <button type="button" className="link-button" disabled={index === 0} onClick={() => patchCombo(c.key, { providers: moved(c.providers, index, -1) })}>
                        {t("settings.up")}
                      </button>
                      <button
                        type="button"
                        className="link-button"
                        disabled={index === c.providers.length - 1}
                        onClick={() => patchCombo(c.key, { providers: moved(c.providers, index, 1) })}
                      >
                        {t("settings.down")}
                      </button>
                      <button type="button" className="link-button skills-danger" onClick={() => patchCombo(c.key, { providers: c.providers.filter((v) => v !== id) })}>
                        {t("settings.remove")}
                      </button>
                    </li>
                  ))}
                </ol>
              )}
              {providerIds.some((id) => !c.providers.includes(id)) && (
                <Field label={t("settings.combos.addProvider")}>
                  <select
                    value=""
                    onChange={(e) => {
                      const id = e.target.value;
                      if (id) patchCombo(c.key, { providers: [...c.providers, id] });
                    }}
                  >
                    <option value="">{t("settings.combos.pick")}</option>
                    {providerIds
                      .filter((id) => !c.providers.includes(id))
                      .map((id) => (
                        <option key={id} value={id}>
                          {id}
                        </option>
                      ))}
                  </select>
                </Field>
              )}
            </li>
          ))}
        </ul>
      </Section>

      <Section
        title={t("settings.policies.title")}
        hint={t("settings.policies.hint")}
        action={
          <button
            type="button"
            className="link-button"
            disabled={modelIds.length === 0}
            onClick={() => update((d) => ({ ...d, modelPolicies: [...d.modelPolicies, keyed({ id: nextPolicyId(delegationCandidates(d.providers, d.combos, d.modelPolicies)), model: modelIds[0] ?? "", description: "" })] }))}
          >
            {t("settings.policies.add")}
          </button>
        }
      >
        {draft.modelPolicies.length === 0 && <p className="skills-hint">{t("settings.policies.none")}</p>}
        <ul className="skills-list">
          {draft.modelPolicies.map((p) => (
            <li key={p.key} className="skills-item settings-card">
              <div className="skills-item-header">
                <span className="skills-hint">{t("settings.policies.policy")}</span>
                <button type="button" className="link-button skills-danger" onClick={() => removePolicy(p.key)}>
                  {t("settings.remove")}
                </button>
              </div>
              <Field label={t("common.name")}>
                <input value={p.id} placeholder={t("settings.policies.namePlaceholder")} onChange={(e) => patchPolicy(p.key, { id: e.target.value })} />
              </Field>
              <Field label={t("settings.policies.answers")}>
                <select value={p.model} onChange={(e) => patchPolicy(p.key, { model: e.target.value })}>
                  {!modelIds.includes(p.model) && <option value={p.model}>{p.model || t("settings.policies.pickModel")}</option>}
                  {modelIds.map((id) => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={t("settings.policies.when")}>
                <input value={p.description ?? ""} maxLength={200} placeholder={t("settings.policies.whenPlaceholder")} onChange={(e) => patchPolicy(p.key, { description: e.target.value })} />
              </Field>
            </li>
          ))}
        </ul>
      </Section>

      <Section
        title={t("settings.agents.title")}
        hint={t("settings.agents.hint")}
        action={
          <button
            type="button"
            className="link-button"
            onClick={() =>
              update((d) => ({
                ...d,
                agents: [
                  ...d.agents,
                  keyed({ id: "", persona: "", providerId: "", canDelegateToAgents: false, canManageAgents: false, canMessageAgents: false, canManageTasks: false, canStartTasks: true, canCreateWorkers: true, canMessageUser: true, canChooseModels: true, allowedTools: null, autonomy: 4, approvalRequired: [] }),
                ],
              }))
            }
          >
            {t("settings.agents.add")}
          </button>
        }
      >
        {draft.agents.length === 0 && <p className="skills-hint">{t("settings.agents.none")}</p>}
        <ul className="skills-list">
          {draft.agents.map((a) => (
            <li key={a.key} className="skills-item settings-card">
              <div className="settings-grid">
                <Field label={t("common.name")}>
                  <input value={a.id} onChange={(e) => patchAgent(a.key, { id: e.target.value })} />
                </Field>
                <Field label={t("settings.agents.defaultModel")}>
                  <select value={a.providerId} onChange={(e) => patchAgent(a.key, { providerId: e.target.value })}>
                    <option value="">{t("settings.agents.ofConversation")}</option>
                    {providerIds.map((id) => (
                      <option key={id} value={id}>
                        {id}
                      </option>
                    ))}
                    {comboIds.map((id) => (
                      <option key={id} value={id}>
                        {t("settings.agents.comboSuffix", { id })}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label={t("settings.agents.role")}>
                  <input value={a.role ?? ""} placeholder={t("settings.agents.rolePlaceholder")} onChange={(e) => patchAgent(a.key, { role: e.target.value })} />
                </Field>
                <Field label={t("settings.agents.reportsTo")}>
                  <select value={a.reportsTo ?? ""} onChange={(e) => patchAgent(a.key, { reportsTo: e.target.value || null })}>
                    <option value="">{t("settings.agents.nobody")}</option>
                    {draft.agents
                      .filter((other) => other.key !== a.key && !descendantsOf(draft.agents, a.id).has(other.id))
                      .map((other) => (
                        <option key={other.key} value={other.id}>
                          {other.id}
                        </option>
                      ))}
                  </select>
                </Field>
                <Field label={t("settings.agents.persona")} wide>
                  <textarea rows={4} value={a.persona} onChange={(e) => patchAgent(a.key, { persona: e.target.value })} />
                </Field>
              </div>
              <div className="settings-checks">
                <label className="settings-check">
                  <input type="checkbox" checked={a.canDelegateToAgents} onChange={(e) => patchAgent(a.key, { canDelegateToAgents: e.target.checked })} />
                  {t("settings.agents.canDelegate")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canManageAgents} onChange={(e) => patchAgent(a.key, { canManageAgents: e.target.checked })} />
                  {t("settings.agents.canManage")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canMessageAgents} onChange={(e) => patchAgent(a.key, { canMessageAgents: e.target.checked })} />
                  {t("settings.agents.canNote")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canManageTasks} onChange={(e) => patchAgent(a.key, { canManageTasks: e.target.checked })} />
                  {t("settings.agents.canSchedule")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canStartTasks ?? true} onChange={(e) => patchAgent(a.key, { canStartTasks: e.target.checked })} />
                  {t("settings.agents.canStart")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canCreateWorkers ?? true} onChange={(e) => patchAgent(a.key, { canCreateWorkers: e.target.checked })} />
                  {t("settings.agents.canWorkers")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canMessageUser ?? true} onChange={(e) => patchAgent(a.key, { canMessageUser: e.target.checked })} />
                  {t("settings.agents.canMessageUser")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={a.canChooseModels ?? true} onChange={(e) => patchAgent(a.key, { canChooseModels: e.target.checked })} />
                  {t("settings.agents.canChoose")}
                </label>
                <label className="settings-check">
                  <input type="checkbox" checked={outreachOn(draft.outreach, a.id)} onChange={(e) => update((d) => ({ ...d, outreach: setOutreach(d.outreach, a.id, e.target.checked) }))} />
                  {t("settings.agents.outreach")}
                </label>
                {outreachOn(draft.outreach, a.id) && (
                  <fieldset className="settings-tools">
                    <legend>{t("settings.agents.forwardLegend")}</legend>
                    {OUTREACH_CHANNELS.map((channel) => (
                      <label key={channel} className="settings-check">
                        <input
                          type="checkbox"
                          checked={outreachForward(draft.outreach, a.id).includes(channel)}
                          onChange={(e) => update((d) => ({ ...d, outreach: setForward(d.outreach, a.id, channel, e.target.checked) }))}
                        />
                        {channel === "telegram" ? t("pairings.telegram") : t("pairings.whatsapp")}
                      </label>
                    ))}
                  </fieldset>
                )}
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={a.allowedTools !== null}
                    onChange={(e) => patchAgent(a.key, { allowedTools: e.target.checked ? [] : null })}
                  />
                  {t("settings.agents.limitTools")}
                </label>
                <label className="settings-check">
                  {t("settings.agents.autonomy")}
                  <select value={a.autonomy ?? 4} onChange={(e) => patchAgent(a.key, { autonomy: Number(e.target.value) })}>
                    <option value={1}>{t("settings.agents.autonomy1")}</option>
                    <option value={2}>{t("settings.agents.autonomy2")}</option>
                    <option value={3}>{t("settings.agents.autonomy3")}</option>
                    <option value={4}>{t("settings.agents.autonomy4")}</option>
                    <option value={5}>{t("settings.agents.autonomy5")}</option>
                  </select>
                </label>
              </div>
              <fieldset className="settings-tools">
                <legend className="skills-hint">{t("settings.agents.approvalBefore")}</legend>
                {APPROVAL_CATEGORIES.map((category) => (
                  <label key={category.id} className="settings-check">
                    <input
                      type="checkbox"
                      checked={(a.approvalRequired ?? []).includes(category.id)}
                      onChange={(e) =>
                        patchAgent(a.key, {
                          approvalRequired: e.target.checked
                            ? [...(a.approvalRequired ?? []), category.id]
                            : (a.approvalRequired ?? []).filter((id) => id !== category.id),
                        })
                      }
                    />
                    {category.label}
                  </label>
                ))}
              </fieldset>
              {(a.canDelegateToAgents || a.canManageAgents || delegationCandidates(draft.providers, draft.combos, draft.modelPolicies).length > 0) && (
                <fieldset className="settings-tools">
                  <legend className="skills-hint">{t("settings.agents.delegationLegend")}</legend>
                  <p className="skills-hint">{delegationSummary(a.delegationModels)}</p>
                  {(a.delegationModels ?? []).length > 0 && (
                    <ol className="fallback-list">
                      {(a.delegationModels ?? []).map((id, index, list) => (
                        <li key={id} className="fallback-row">
                          <span className="fallback-name">{id}</span>
                          <button type="button" className="link-button" disabled={index === 0} onClick={() => patchAgent(a.key, { delegationModels: moved(list, index, -1) })}>
                            {t("settings.up")}
                          </button>
                          <button type="button" className="link-button" disabled={index === list.length - 1} onClick={() => patchAgent(a.key, { delegationModels: moved(list, index, 1) })}>
                            {t("settings.down")}
                          </button>
                          <button type="button" className="link-button skills-danger" onClick={() => patchAgent(a.key, { delegationModels: list.filter((m) => m !== id) })}>
                            {t("settings.remove")}
                          </button>
                        </li>
                      ))}
                    </ol>
                  )}
                  {delegationCandidates(draft.providers, draft.combos, draft.modelPolicies).some((id) => !(a.delegationModels ?? []).includes(id)) && (
                    <select
                      value=""
                      aria-label={t("settings.agents.limitToModel")}
                      onChange={(e) => {
                        const id = e.target.value;
                        if (id) patchAgent(a.key, { delegationModels: [...(a.delegationModels ?? []), id] });
                      }}
                    >
                      <option value="">{t("settings.agents.limitToModelOption")}</option>
                      {delegationCandidates(draft.providers, draft.combos, draft.modelPolicies)
                        .filter((id) => !(a.delegationModels ?? []).includes(id))
                        .map((id) => (
                          <option key={id} value={id}>
                            {id}
                          </option>
                        ))}
                    </select>
                  )}
                </fieldset>
              )}
              {people.length > 0 && (
                <fieldset className="settings-tools">
                  <legend className="skills-hint">{t("settings.agents.shareWith")}</legend>
                  <label className="settings-check">
                    <input
                      type="checkbox"
                      checked={(a.sharedWith ?? []).includes("*")}
                      onChange={(e) => patchAgent(a.key, { sharedWith: e.target.checked ? ["*"] : [] })}
                    />
                    {t("settings.agents.allPeople")}
                  </label>
                  {!(a.sharedWith ?? []).includes("*") &&
                    people.map((p) => (
                      <label key={p.id} className="settings-check">
                        <input
                          type="checkbox"
                          checked={(a.sharedWith ?? []).includes(p.id)}
                          onChange={(e) =>
                            patchAgent(a.key, {
                              sharedWith: e.target.checked ? [...(a.sharedWith ?? []), p.id] : (a.sharedWith ?? []).filter((s) => s !== p.id),
                            })
                          }
                        />
                        {p.name} <code>{p.id}</code>
                      </label>
                    ))}
                </fieldset>
              )}
              {a.allowedTools !== null && (
                <fieldset className="settings-tools">
                  <legend className="skills-hint">{t("settings.agents.onlyTools")}</legend>
                  {settings.toolNames.map((tool) => (
                    <label key={tool} className="settings-check">
                      <input
                        type="checkbox"
                        checked={a.allowedTools!.includes(tool)}
                        onChange={(e) =>
                          patchAgent(a.key, {
                            allowedTools: e.target.checked ? [...a.allowedTools!, tool] : a.allowedTools!.filter((x) => x !== tool),
                          })
                        }
                      />
                      <code>{tool}</code>
                    </label>
                  ))}
                </fieldset>
              )}
              <div className="skills-actions">
                <button type="button" className="link-button skills-danger" onClick={() => update((d) => ({ ...d, agents: removeFromOrg(d.agents, a.id), outreach: dropOutreach(d.outreach, a.id) }))}>
                  {t("settings.agents.removeAgent")}
                </button>
              </div>
            </li>
          ))}
        </ul>
      </Section>

      <Section title={t("settings.keys.title")} hint={t("settings.keys.hint")}>
        <div className="settings-grid">
          <SecretField
            label={t("settings.keys.tavily")}
            value={draft.tavilyKey}
            writable={secretsWritable}
            onChange={(edit) => update((d) => ({ ...d, tavilyKey: { ...d.tavilyKey, edit } }))}
          />
          <SecretField
            label={t("settings.keys.whisper")}
            value={draft.whisperKey}
            writable={secretsWritable}
            onChange={(edit) => update((d) => ({ ...d, whisperKey: { ...d.whisperKey, edit } }))}
          />
        </div>
      </Section>

      <Section title={t("settings.git.title")} hint={t("settings.git.hint")}>
        <div className="settings-grid">
          <Field label={t("settings.git.url")} hint={t("settings.git.urlHint")}>
            <input
              value={draft.gitRemoteUrl}
              placeholder={t("settings.git.urlPlaceholder")}
              onChange={(e) => {
                const gitRemoteUrl = e.target.value;
                update((d) => ({ ...d, gitRemoteUrl }));
              }}
            />
          </Field>
          <SecretField
            label={t("settings.git.token")}
            value={draft.gitToken}
            writable={secretsWritable}
            onChange={(edit) => update((d) => ({ ...d, gitToken: { ...d.gitToken, edit } }))}
          />
        </div>
      </Section>

      <Section title={t("settings.bots.title")} hint={t("settings.bots.hint")}>
        {botsProblem && <p className="error-banner">{botsProblem}</p>}
        <div className="settings-checks">
          <label className="settings-check">
            <input type="checkbox" checked={draft.botsLearningEnabled} onChange={(e) => update((d) => ({ ...d, botsLearningEnabled: e.target.checked }))} />
            {t("settings.bots.learn")}
          </label>
        </div>
        <div className="settings-checks">
          <label className="settings-check">
            <input type="checkbox" checked={draft.botsTelegramPairing} onChange={(e) => update((d) => ({ ...d, botsTelegramPairing: e.target.checked }))} />
            {t("settings.bots.telegramPairing")}
          </label>
          <label className="settings-check">
            <input type="checkbox" checked={draft.botsWhatsappPairing} onChange={(e) => update((d) => ({ ...d, botsWhatsappPairing: e.target.checked }))} />
            {t("settings.bots.whatsappPairing")}
          </label>
        </div>
        <BotPairings conn={conn} disabled={dirty} onResolved={load} />
        <div className="settings-grid">
          <SecretField
            label={t("settings.bots.telegramToken")}
            placeholder={t("settings.bots.pasteToken")}
            value={draft.telegramToken}
            writable={secretsWritable}
            onChange={(edit) => update((d) => ({ ...d, telegramToken: { ...d.telegramToken, edit } }))}
          />
          <p className="field-hint settings-field--wide">{t("settings.bots.tokenNote")}</p>
          <Field label={t("settings.bots.learningModel")} hint={t("settings.bots.learningModelHint")}>
            <select value={draft.botsLearningProvider} onChange={(e) => update((d) => ({ ...d, botsLearningProvider: e.target.value }))}>
              <option value="">{t("settings.bots.activeModel")}</option>
              {modelIds.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
          </Field>
          <Field label={t("settings.bots.perDay")} hint={t("settings.bots.perDayHint")}>
            <input type="number" min={1} step={1} value={draft.botsMaxPerDay} onChange={(e) => update((d) => ({ ...d, botsMaxPerDay: e.target.value }))} />
          </Field>
          <Field label={t("settings.bots.telegramUsers")} hint={t("settings.bots.telegramUsersHint")} wide>
            <textarea rows={3} value={draft.botsTelegramUsers} onChange={(e) => update((d) => ({ ...d, botsTelegramUsers: e.target.value }))} />
          </Field>
          <Field label={t("settings.bots.whatsappChats")} hint={t("settings.bots.whatsappChatsHint")} wide>
            <textarea rows={3} value={draft.botsWhatsappChats} onChange={(e) => update((d) => ({ ...d, botsWhatsappChats: e.target.value }))} />
          </Field>
          <Field label={t("settings.bots.learningChats")} hint={t("settings.bots.learningChatsHint")} wide>
            <textarea rows={3} value={draft.botsLearningChats} onChange={(e) => update((d) => ({ ...d, botsLearningChats: e.target.value }))} />
          </Field>
        </div>
      </Section>

      <Section title={t("settings.risk.title")} hint={t("settings.risk.hint")}>
        {draft.toolCategories.length === 0 && <p className="muted">{t("settings.risk.none")}</p>}
        <ul className="skills-list">
          {draft.toolCategories.map((entry, i) => (
            <li key={i} className="skills-item settings-card">
              <div className="settings-grid">
                <Field label={t("settings.risk.tool")}>
                  <input
                    value={entry.tool}
                    placeholder={t("settings.risk.toolPlaceholder")}
                    onChange={(e) => update((d) => ({ ...d, toolCategories: d.toolCategories.map((c, j) => (j === i ? { ...c, tool: e.target.value } : c)) }))}
                  />
                </Field>
                <Field label={t("settings.risk.kind")}>
                  <select
                    value={entry.category}
                    onChange={(e) => update((d) => ({ ...d, toolCategories: d.toolCategories.map((c, j) => (j === i ? { ...c, category: e.target.value } : c)) }))}
                  >
                    {APPROVAL_CATEGORIES.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.label}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
              <button type="button" className="link-button skills-danger" onClick={() => update((d) => ({ ...d, toolCategories: d.toolCategories.filter((_, j) => j !== i) }))}>
                {t("settings.remove")}
              </button>
            </li>
          ))}
        </ul>
        <button
          type="button"
          className="link-button"
          onClick={() => update((d) => ({ ...d, toolCategories: [...d.toolCategories, { tool: "", category: APPROVAL_CATEGORIES[0].id }] }))}
        >
          {t("settings.risk.add")}
        </button>
      </Section>

      {draft.grantsSupported && (
        <Section title={t("settings.grants.title")} hint={t("settings.grants.hint")}>
          <div className="settings-checks">
            <label className="settings-check">
              <input type="checkbox" checked={draft.persistGrants} onChange={(e) => update((d) => ({ ...d, persistGrants: e.target.checked }))} />
              {t("settings.grants.keep")}
            </label>
          </div>
          <Field label={t("settings.grants.days")} hint={t("settings.grants.daysHint")}>
            <input
              type="number"
              min={1}
              step={1}
              value={draft.grantTtlDays}
              disabled={!draft.persistGrants}
              onChange={(e) => update((d) => ({ ...d, grantTtlDays: e.target.value }))}
            />
          </Field>
        </Section>
      )}

      <Section title={t("settings.limits.title")} hint={t("settings.limits.hint")}>
        {settings.limitsDisabledByEnv && <p className="banner settings-note">{t("settings.limits.envOff")}</p>}
        <div className="settings-checks" role="radiogroup" aria-label={t("settings.limits.label")}>
          {(
            [
              ["default", t("settings.limits.default")],
              ["custom", t("settings.limits.custom")],
              ["off", t("settings.limits.off")],
            ] as [LimitsMode, string][]
          ).map(([mode, label]) => (
            <label key={mode} className="settings-check">
              <input type="radio" name="limits-mode" checked={draft.limitsMode === mode} onChange={() => setLimitsMode(mode)} />
              {label}
            </label>
          ))}
        </div>
        {draft.limitsMode === "custom" && (
          <>
            <ul className="skills-list">
              {draft.limits.map((l) => {
                const scopeList = scopes();
                const scope = scopeList.find((s) => s.value === l.scope) ?? scopeList[0];
                return (
                  <li key={l.key} className="skills-item settings-card">
                    <div className="settings-grid">
                      <Field label={t("common.name")}>
                        <input value={l.id} onChange={(e) => patchLimit(l.key, { id: e.target.value })} />
                      </Field>
                      <Field label={t("settings.limits.appliesTo")}>
                        <select
                          value={l.scope}
                          onChange={(e) => patchLimit(l.key, { scope: e.target.value as LimitScope, target: e.target.value === "global" ? "" : l.target })}
                        >
                          {scopeList.map((s) => (
                            <option key={s.value} value={s.value}>
                              {s.label}
                            </option>
                          ))}
                        </select>
                      </Field>
                      {l.scope !== "global" && (
                        <Field label={t("settings.limits.who")}>
                          <input value={l.target} placeholder={scope.target} onChange={(e) => patchLimit(l.key, { target: e.target.value })} />
                        </Field>
                      )}
                      <Field label={t("settings.limits.window")}>
                        <input type="number" min={1} value={l.windowHours} onChange={(e) => patchLimit(l.key, { windowHours: Number(e.target.value) })} />
                      </Field>
                      <Field label={t("settings.limits.maxTokens")}>
                        <input type="number" min={1} value={l.maxTokens ?? ""} onChange={(e) => patchLimit(l.key, { maxTokens: numberOrNull(e.target.value) })} />
                      </Field>
                      <Field label={t("settings.limits.maxUsd")} hint={t("settings.limits.maxUsdHint")}>
                        <input
                          type="number"
                          min={0}
                          step="0.01"
                          value={l.maxCostUsd ?? ""}
                          onChange={(e) => patchLimit(l.key, { maxCostUsd: numberOrNull(e.target.value) })}
                        />
                      </Field>
                    </div>
                    <div className="skills-actions">
                      <button type="button" className="link-button skills-danger" onClick={() => update((d) => ({ ...d, limits: d.limits.filter((x) => x.key !== l.key) }))}>
                        {t("settings.limits.removeLimit")}
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
            <button
              type="button"
              className="link-button settings-add"
              onClick={() =>
                update((d) => ({
                  ...d,
                  limits: [
                    ...d.limits,
                    keyed({ id: "", scope: "global" as LimitScope, target: "", windowHours: 24, maxTokens: null, maxCostUsd: null, warnAt: null, extendStep: null }),
                  ],
                }))
              }
            >
              {t("settings.limits.add")}
            </button>
          </>
        )}
      </Section>

      <Section
        title={t("settings.prices.title")}
        hint={t("settings.prices.hint")}
        action={
          <button type="button" className="link-button" onClick={() => update((d) => ({ ...d, prices: [...d.prices, keyed({ model: "", inputPerMtok: 0, outputPerMtok: 0 })] }))}>
            {t("settings.prices.add")}
          </button>
        }
      >
        {draft.prices.length === 0 && <p className="skills-hint">{t("settings.prices.none")}</p>}
        <ul className="skills-list">
          {draft.prices.map((p) => (
            <li key={p.key} className="skills-item settings-card settings-price">
              <Field label={t("settings.prices.model")}>
                <input value={p.model} onChange={(e) => patchPrice(p.key, { model: e.target.value })} />
              </Field>
              <Field label={t("settings.prices.input")}>
                <input type="number" min={0} step="0.01" value={p.inputPerMtok} onChange={(e) => patchPrice(p.key, { inputPerMtok: Number(e.target.value) })} />
              </Field>
              <Field label={t("settings.prices.output")}>
                <input type="number" min={0} step="0.01" value={p.outputPerMtok} onChange={(e) => patchPrice(p.key, { outputPerMtok: Number(e.target.value) })} />
              </Field>
              <button
                type="button"
                className="link-button skills-danger settings-price-remove"
                aria-label={t("settings.prices.removeLabel", { model: p.model || t("settings.prices.unnamed") })}
                onClick={() => update((d) => ({ ...d, prices: d.prices.filter((x) => x.key !== p.key) }))}
              >
                {t("settings.remove")}
              </button>
            </li>
          ))}
        </ul>
      </Section>

      <AdvancedSection draft={draft.advanced} error={advancedProblem} onChange={(change) => update((d) => ({ ...d, advanced: { ...d.advanced, ...change } }))} />

      <MachineSection
        draft={draft.machine}
        settings={settings.machine}
        agentIds={agentIds}
        secretsWritable={secretsWritable}
        error={machineProblem}
        conn={conn}
        onChange={(change) => update((d) => ({ ...d, machine: change(d.machine) }))}
      />

      <div className="settings-footer">
        {saveError && (
          <p className="error-banner">
            {saveError.text}
            {saveError.conflict && (
              <>
                {" "}
                <button type="button" className="link-button" onClick={load}>
                  {t("settings.reload")}
                </button>
              </>
            )}
          </p>
        )}
        {savedAt !== null && !dirty && <p className="settings-saved">{t("settings.saved")}</p>}
        {confirmingMachine ? (
          <div className="settings-confirm settings-confirm--danger" role="alertdialog" aria-labelledby="machine-confirm-title">
            <h3 id="machine-confirm-title" className="settings-subheading">
              {t("settings.machineConfirmTitle")}
            </h3>
            <ul className="settings-changes">
              {machineSummary(draft.machineBase, toMachineEdit(draft.machine)).map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
            <p className="field-hint">{t("settings.machineConfirmNote")}</p>
            <label className="settings-check">
              <input type="checkbox" checked={machineAck} onChange={(e) => setMachineAck(e.target.checked)} />
              {t("settings.machineAck")}
            </label>
            <div className="skills-actions">
              <button
                type="button"
                className="primary-button"
                disabled={!machineAck}
                onClick={() => {
                  setConfirmingMachine(false);
                  setAsking(true);
                }}
              >
                {t("settings.continue")}
              </button>
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setConfirmingMachine(false);
                  setMachineAck(false);
                }}
              >
                {t("common.cancel")}
              </button>
            </div>
          </div>
        ) : asking ? (
          <form
            className="settings-confirm"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <Field label={t("pairingKey.label")} hint={t("settings.pairingHint")}>
              <input type="password" autoComplete="current-password" autoFocus value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
            </Field>
            {keyError && <p className="error-banner">{keyError}</p>}
            <div className="skills-actions">
              <button type="submit" className="primary-button" disabled={saving || pairingKey.trim() === "" || !conn}>
                {saving ? t("settings.savingRestarting") : t("common.confirm")}
              </button>
              <button
                type="button"
                className="link-button"
                disabled={saving}
                onClick={() => {
                  setAsking(false);
                  setPairingKey("");
                  setKeyError(null);
                }}
              >
                {t("common.cancel")}
              </button>
            </div>
          </form>
        ) : (
          <div className="skills-actions">
            <button
              type="button"
              className="primary-button"
              disabled={!dirty || !conn || botsProblem !== null || machineProblem !== null || advancedProblem !== null}
              onClick={() => {
                // What reaches the machine is read back first; the pairing key comes after.
                if (machineIsChanged) setConfirmingMachine(true);
                else setAsking(true);
              }}
            >
              {t("settings.save")}
            </button>
            <button type="button" className="link-button" disabled={!dirty} onClick={() => setDraft(toDraft(settings))}>
              {t("settings.discard")}
            </button>
          </div>
        )}
      </div>

      {/* P12 — saved on its own, not with the form above. */}
      <ApiKeysSection conn={conn} />
    </div>
  );
}
