import { useCallback, useEffect, useState } from "react";
import { UserError, type ServerConnection } from "../hub/connection";
import type { NodeFolder, RemovedUser, UserInfo } from "../hub/messages";
import { ORG_ACCESS_CHOICES, orgAccessLabel, orgAccessOf, type OrgAccess } from "../hub/org";
import { t } from "../i18n";
import { rich } from "../i18n/rich";
import RecoveryPolicySection from "./RecoveryPolicySection";
import SharedSpacesSection from "./SharedSpacesSection";

// People (P84): the members of this workspace besides the owner. Each signs in with a username and
// password, and has their own vault and conversations on this hub. Every change asks for the pairing
// key, like approving a device. A provisional password is shown once, right after it's created.

type Asking =
  | { kind: "create"; id: string; name: string }
  | { kind: "rename"; user: UserInfo; name: string }
  | { kind: "reset"; user: UserInfo }
  | { kind: "remove"; user: UserInfo }
  | { kind: "restore"; user: RemovedUser }
  | { kind: "invite"; user: UserInfo }
  | { kind: "unlink"; user: UserInfo }
  /** `null`: back to the safe default. */
  | { kind: "tools"; user: UserInfo; tools: string[] | null }
  /** P115: `""` is the workspace's own model. */
  | { kind: "learning"; user: UserInfo; provider: string }
  /** P100: the providers and combos they may give their own agents; none is only the hub's default model. */
  | { kind: "models"; user: UserInfo; models: string[] }
  /** P120: what they do with the organization of the agents. */
  | { kind: "orgAccess"; user: UserInfo; access: OrgAccess }
  /** P102: the folders they may work in, one per line — of the hub's machine, and on nodes as `<node id>:<path>`. */
  | { kind: "folders"; user: UserInfo; workdirs: string; nodeWorkdirs: string };

/** `node-id:path` lines → the entries; a line with no `:` is a mistake the hub would also refuse. */
function parseNodeLines(text: string): NodeFolder[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      const colon = line.indexOf(":");
      return colon < 0 ? { node: line, path: "" } : { node: line.slice(0, colon).trim(), path: line.slice(colon + 1).trim() };
    });
}

const lines = (text: string): string[] => text.split("\n").map((l) => l.trim()).filter(Boolean);

/** Which folders a person may pick, in a few words. */
function foldersLabel(user: UserInfo): string {
  const count = (user.workdirs?.length ?? 0) + (user.nodeWorkdirs?.length ?? 0);
  return count === 0 ? t("people.noFolders") : t(count === 1 ? "people.foldersOne" : "people.foldersMany", { count });
}

/** The button that opens the models a person may give their agents, with how many are released. */
function modelsLabel(user: UserInfo): string {
  const count = user.models?.length ?? 0;
  return count === 0 ? t("people.modelsDefault") : t(count === 1 ? "people.modelsOne" : "people.modelsMany", { count });
}

/** Mirrors `warden_bootstrap::users::default_member_tool`: what a member has when you never chose. */
function safeByDefault(tool: string): boolean {
  return ["read_file", "write_file", "use_skill", "read_skill_file", "manage_skill", "delegate_task", "jobs", "budget", "generate_document", "search_history"].includes(tool) || tool.startsWith("tavily");
}

/** Mirrors `NEVER_FOR_MEMBERS`: tools a member never gets, so they aren't offered. */
const NEVER_FOR_MEMBERS = ["delegate_to_agent", "message_agent", "manage_agents", "manage_tasks", "usage_stats"];

/** What each person's tools reach, in a few words. */
function toolsLabel(user: UserInfo): string {
  if (user.tools == null) return t("people.toolsDefault");
  if (user.tools.length === 0) return t("people.toolsNone");
  return t("people.tools", { tools: user.tools.join(", ") });
}

/** P84 slice 4: whether a person's data is encrypted on the hub — the owner sees the state, never the data. */
function dataLabel(user: UserInfo): string {
  if (!user.encrypted) return t("people.dataPlain");
  if (user.needsRecovery) return t("people.dataNeedsCode");
  return t("people.dataEncrypted");
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function PeopleView({ conn }: { conn: ServerConnection | null }) {
  const [users, setUsers] = useState<UserInfo[] | null>(null);
  /** Removed members whose encrypted data the hub kept (P84 slice 4). */
  const [removed, setRemoved] = useState<RemovedUser[]>([]);
  /** The workspace's recovery policy (P84 slice 4 parte B). */
  const [recoveryPolicy, setRecoveryPolicy] = useState("private");
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The provisional password to hand over, and to whom — shown once. */
  const [shown, setShown] = useState<{ id: string; password: string } | null>(null);
  /** An invite to link a TruthID, and for whom — shown once. */
  const [invite, setInvite] = useState<{ id: string; code: string } | null>(null);
  /** The hub's tools, for choosing each person's. */
  const [toolNames, setToolNames] = useState<string[]>([]);
  /** The hub's models and combos, for choosing the one each person's learning uses. */
  const [modelIds, setModelIds] = useState<string[]>([]);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      const list = await conn.listUsers();
      setUsers(list.users);
      setRemoved(list.removed);
      setRecoveryPolicy(list.recoveryPolicy ?? "private");
      setError(null);
    } catch (err) {
      setError(message(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
    conn
      ?.requestSettings()
      .then(({ settings }) => {
        setToolNames(settings.toolNames.filter((name) => !NEVER_FOR_MEMBERS.includes(name)));
        setModelIds([...settings.providers.map((p) => p.id), ...settings.combos.map((c) => c.id)]);
      })
      .catch(() => {
        setToolNames([]);
        setModelIds([]);
      });
  }, [conn, load]);

  function cancel() {
    setAsking(null);
    setPairingKey("");
    setKeyError(null);
  }

  async function confirm() {
    if (!conn || !asking) return;
    setBusy(true);
    setKeyError(null);
    try {
      const reply =
        asking.kind === "create"
          ? await conn.saveUser(pairingKey, asking.id.trim().toLowerCase(), asking.name.trim(), true)
          : asking.kind === "rename"
            ? await conn.saveUser(pairingKey, asking.user.id, asking.name.trim(), false)
            : asking.kind === "reset"
              ? await conn.resetPassword(pairingKey, asking.user.id)
              : asking.kind === "tools"
                ? await conn.setUserTools(pairingKey, asking.user.id, asking.tools)
                : asking.kind === "learning"
                  ? await conn.setUserLearningProvider(pairingKey, asking.user.id, asking.provider || null)
                : asking.kind === "models"
                  ? await conn.setUserModels(pairingKey, asking.user.id, asking.models)
                : asking.kind === "orgAccess"
                  ? await conn.setUserOrgAccess(pairingKey, asking.user.id, asking.access)
                : asking.kind === "folders"
                  ? await conn.setUserWorkdirs(pairingKey, asking.user.id, lines(asking.workdirs), parseNodeLines(asking.nodeWorkdirs))
                : asking.kind === "restore"
                  ? await conn.restoreUser(pairingKey, asking.user.id)
                  : asking.kind === "invite"
                  ? await conn.createInvite(pairingKey, asking.user.id)
                  : asking.kind === "unlink"
                    ? await conn.unlinkTruthId(pairingKey, asking.user.id)
                    : await conn.removeUser(pairingKey, asking.user.id);
      setUsers(reply.users);
      setRemoved(reply.removed);
      if (reply.inviteCode && asking.kind === "invite") setInvite({ id: asking.user.id, code: reply.inviteCode });
      if (reply.tempPassword) {
        setShown({ id: asking.kind === "create" ? asking.id.trim().toLowerCase() : asking.kind === "reset" ? asking.user.id : "", password: reply.tempPassword });
      }
      cancel();
    } catch (err) {
      if (err instanceof UserError && err.authRejected) {
        setKeyError(t("pairingKey.wrong"));
      } else {
        setKeyError(message(err));
      }
    } finally {
      setBusy(false);
    }
  }

  const keyForm = (label: string, danger = false, extra?: React.ReactNode) => (
    <form
      className="settings-confirm devices-confirm"
      onSubmit={(e) => {
        e.preventDefault();
        void confirm();
      }}
    >
      {extra}
      <label className="settings-field">
        {t("pairingKey.label")}
        <input type="password" autoComplete="current-password" value={pairingKey} onChange={(e) => setPairingKey(e.target.value)} />
        <span className="field-hint">{t("people.keyHint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className={danger ? "primary-button skills-danger" : "primary-button"} disabled={busy || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : label}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancel}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  return (
    <div className="skills-view">
      <div className="skills-header">
        <h2>{t("people.title")}</h2>
        <button type="button" className="primary-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "create", id: "", name: "" })}>
          {t("people.add")}
        </button>
      </div>
      <p className="skills-hint">{t("people.intro")}</p>
      {error && <p className="error-banner">{error}</p>}

      {shown && (
        <div className="settings-confirm">
          <p>{rich("people.tempPassword", { id: <strong>{shown.id}</strong>, password: <code>{shown.password}</code> })}</p>
          <p className="skills-hint">{t("people.tempHint")}</p>
          <button type="button" className="link-button" onClick={() => setShown(null)}>
            {t("people.noted")}
          </button>
        </div>
      )}

      {invite && (
        <div className="settings-confirm">
          <p>{rich("people.inviteShown", { id: <strong>{invite.id}</strong>, code: <code>{invite.code}</code> })}</p>
          <p className="skills-hint">{t("people.inviteHint")}</p>
          <button type="button" className="link-button" onClick={() => setInvite(null)}>
            {t("people.noted")}
          </button>
        </div>
      )}

      {asking?.kind === "create" &&
        keyForm(
          t("people.create"),
          false,
          <>
            <label className="settings-field">
              {t("people.user")}
              <input
                autoCapitalize="none"
                value={asking.id}
                onChange={(e) => setAsking({ ...asking, id: e.target.value })}
                placeholder={t("people.userPlaceholder")}
                autoFocus
              />
              <span className="field-hint">{t("people.userHint")}</span>
            </label>
            <label className="settings-field">
              {t("common.name")}
              <input value={asking.name} onChange={(e) => setAsking({ ...asking, name: e.target.value })} placeholder={t("people.namePlaceholder")} />
            </label>
          </>,
        )}

      {users === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : users.length === 0 ? (
        <p className="skills-hint">{t("people.onlyYou")}</p>
      ) : (
        <ul className="skills-list">
          {users.map((user) => {
            const mine = asking !== null && asking.kind !== "create" && asking.user.id === user.id ? asking : null;
            return (
              <li key={user.id} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">{user.name}</span>
                  <span className={`devices-status devices-status--${user.mustChangePassword ? "pending" : "approved"}`}>
                    {user.mustChangePassword ? t("people.tempPasswordBadge") : t("people.active")}
                  </span>
                </div>
                <p className="skills-item-description">
                  <code>{user.id}</code> · {toolsLabel(user)}
                  {user.agents.length > 0 && t("people.ownAgents", { agents: user.agents.join(", ") })}
                  {` · ${dataLabel(user)}`}
                  {` · ${foldersLabel(user)}`}
                  {user.learningProvider ? t("people.learningWith", { provider: user.learningProvider }) : ""}
                  {` · ${orgAccessLabel(orgAccessOf(user.orgAccess))}`}
                  {user.truthid ? t("people.truthIdLinked", { name: user.truthid }) : user.inviteOpen ? t("people.inviteOpen") : ""}
                </p>
                {mine?.kind === "rename" &&
                  keyForm(
                    t("people.save"),
                    false,
                    <label className="settings-field">
                      {t("common.name")}
                      <input value={mine.name} onChange={(e) => setAsking({ ...mine, name: e.target.value })} autoFocus />
                    </label>,
                  )}
                {mine?.kind === "reset" && keyForm(t("people.genTemp"))}
                {mine?.kind === "invite" && keyForm(t("people.genInvite"))}
                {mine?.kind === "unlink" && keyForm(t("people.unlink"), true)}
                {mine?.kind === "tools" &&
                  keyForm(
                    t("people.saveTools"),
                    false,
                    <fieldset className="settings-tools">
                      <label className="settings-check">
                        <input type="checkbox" checked={mine.tools === null} onChange={(e) => setAsking({ ...mine, tools: e.target.checked ? null : toolNames.filter(safeByDefault) })} />
                        {t("people.useSafeDefault")}
                      </label>
                      {mine.tools !== null &&
                        toolNames.map((tool) => (
                          <label key={tool} className="settings-check">
                            <input
                              type="checkbox"
                              checked={mine.tools!.includes(tool)}
                              onChange={(e) => setAsking({ ...mine, tools: e.target.checked ? [...mine.tools!, tool] : mine.tools!.filter((x) => x !== tool) })}
                            />
                            <code>{tool}</code>
                            {!safeByDefault(tool) && <span className="skills-danger">{t("people.reachesOwn")}</span>}
                          </label>
                        ))}
                      <span className="field-hint">{t("people.toolsCap")}</span>
                    </fieldset>,
                  )}
                {mine?.kind === "folders" &&
                  keyForm(
                    t("people.saveFolders"),
                    false,
                    <>
                      <label className="settings-field">
                        {t("people.hubFolders", { name: user.name })}
                        <textarea
                          rows={3}
                          value={mine.workdirs}
                          onChange={(e) => setAsking({ ...mine, workdirs: e.target.value })}
                          placeholder={t("people.foldersPlaceholder")}
                          autoFocus
                        />
                        <span className="field-hint">{t("people.hubFoldersHint")}</span>
                      </label>
                      <label className="settings-field">
                        {t("people.nodeFolders")}
                        <textarea
                          rows={3}
                          value={mine.nodeWorkdirs}
                          onChange={(e) => setAsking({ ...mine, nodeWorkdirs: e.target.value })}
                          placeholder={t("people.nodeFoldersPlaceholder")}
                        />
                        <span className="field-hint">{rich("people.nodeFoldersHint", { fmt: <code>{t("people.nodeFoldersFormat")}</code> })}</span>
                      </label>
                      <span className="field-hint">{t("people.foldersNote", { name: user.name })}</span>
                    </>,
                  )}
                {mine?.kind === "learning" &&
                  keyForm(
                    t("people.saveModel"),
                    false,
                    <label className="settings-field">
                      {t("people.learningModel", { name: user.name })}
                      <select value={mine.provider} onChange={(e) => setAsking({ ...mine, provider: e.target.value })}>
                        <option value="">{t("people.workspaceDefault")}</option>
                        {/* A model the hub no longer has stays listed, so saving doesn't silently swap it. */}
                        {mine.provider && !modelIds.includes(mine.provider) && <option value={mine.provider}>{t("people.noLongerExists", { id: mine.provider })}</option>}
                        {modelIds.map((id) => (
                          <option key={id} value={id}>
                            {id}
                          </option>
                        ))}
                      </select>
                      <span className="field-hint">{t("people.learningHint")}</span>
                    </label>,
                  )}
                {mine?.kind === "models" &&
                  keyForm(
                    t("people.saveModels"),
                    false,
                    <fieldset className="settings-tools">
                      <legend>{t("people.modelsLegend", { name: user.name })}</legend>
                      {modelIds.length === 0 && <span className="field-hint">{t("people.noProviders")}</span>}
                      {modelIds.map((id) => (
                        <label key={id} className="settings-check">
                          <input
                            type="checkbox"
                            checked={mine.models.includes(id)}
                            onChange={(e) => setAsking({ ...mine, models: e.target.checked ? [...mine.models, id] : mine.models.filter((m) => m !== id) })}
                          />
                          <code>{id}</code>
                        </label>
                      ))}
                      <span className="field-hint">{t("people.modelsHint", { name: user.name })}</span>
                    </fieldset>,
                  )}
                {mine?.kind === "orgAccess" &&
                  keyForm(
                    t("people.saveAccess"),
                    false,
                    <fieldset className="settings-field">
                      <legend>{t("people.orgLegend", { name: user.name })}</legend>
                      {ORG_ACCESS_CHOICES.map((choice) => (
                        <label key={choice.value} className="settings-check">
                          <input type="radio" name={`org-${user.id}`} checked={mine.access === choice.value} onChange={() => setAsking({ ...mine, access: choice.value })} />
                          {choice.label}
                          <span className="field-hint"> {choice.hint}</span>
                        </label>
                      ))}
                      <span className="field-hint">{t("people.orgHint")}</span>
                    </fieldset>,
                  )}
                {mine?.kind === "remove" &&
                  keyForm(
                    t("people.remove"),
                    true,
                    <p className="error-banner">
                      {t("people.removeWarning", { name: user.name })} {user.encrypted ? t("people.removeEncrypted") : t("people.removePlain")}
                    </p>,
                  )}
                {!mine && (
                  <div className="skills-actions">
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "rename", user, name: user.name })}>
                      {t("people.rename")}
                    </button>
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "tools", user, tools: user.tools ?? null })}>
                      {t("people.toolsButton")}
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      disabled={!conn || asking !== null}
                      onClick={() =>
                        setAsking({
                          kind: "folders",
                          user,
                          workdirs: (user.workdirs ?? []).join("\n"),
                          nodeWorkdirs: (user.nodeWorkdirs ?? []).map((f) => `${f.node}:${f.path}`).join("\n"),
                        })
                      }
                    >
                      {t("people.workFolders")}
                    </button>
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "models", user, models: user.models ?? [] })}>
                      {modelsLabel(user)}
                    </button>
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "learning", user, provider: user.learningProvider ?? "" })}>
                      {t("people.learningButton")}
                    </button>
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "orgAccess", user, access: orgAccessOf(user.orgAccess) })}>
                      {t("people.orgButton")}
                    </button>
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "reset", user })}>
                      {t("people.newTemp")}
                    </button>
                    {user.truthid ? (
                      <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "unlink", user })}>
                        {t("people.unlink")}
                      </button>
                    ) : (
                      <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "invite", user })}>
                        {t("people.inviteToTruthId")}
                      </button>
                    )}
                    {!user.truthid && user.inviteOpen && (
                      <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "unlink", user })}>
                        {t("people.cancelInvite")}
                      </button>
                    )}
                    <button type="button" className="link-button skills-danger" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "remove", user })}>
                      {t("people.remove")}
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {removed.length > 0 && (
        <>
          <h3>{t("people.removedTitle")}</h3>
          <p className="skills-hint">{rich("people.removedIntro", { cmd: <code>warden-server users purge</code> })}</p>
          <ul className="skills-list">
            {removed.map((gone) => (
              <li key={gone.id} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">{gone.name}</span>
                  <span className="devices-status devices-status--pending">{t("people.removedBadge")}</span>
                </div>
                <p className="skills-item-description">
                  <code>{gone.id}</code>
                </p>
                {asking?.kind === "restore" && asking.user.id === gone.id ? (
                  keyForm(t("people.restore"))
                ) : (
                  <div className="skills-actions">
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "restore", user: gone })}>
                      {t("people.restore")}
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        </>
      )}

      <SharedSpacesSection conn={conn} users={users ?? []} />

      <RecoveryPolicySection
        conn={conn}
        users={users ?? []}
        policy={recoveryPolicy}
        onChanged={(next, policy, temp) => {
          if (next) setUsers(next);
          setRecoveryPolicy(policy);
          if (temp) setShown(temp);
        }}
      />
    </div>
  );
}
