import type { ServerConnection } from "../hub/connection";
import type { MachineSettings } from "../hub/messages";
import { t } from "../i18n";
import SshAuditLog from "./SshAuditLog";
import { newEntry, newMcp, newSsh, type McpDraft, type McpEntryDraft, type MachineDraft, type SshDraft } from "./machineDraft";
import { Field, SecretField, Section } from "./settingsParts";

// "Hub machine" (P119): what reaches the computer the hub runs on. Every control sits in one disabled
// <fieldset> while the hub doesn't let a save change it, so a locked screen can't even look editable.

interface Props {
  draft: MachineDraft;
  /** What the hub sent, for the lock and its reason. */
  settings: MachineSettings;
  agentIds: string[];
  secretsWritable: boolean;
  error: string | null;
  /** The hub's connection, for the SSH usage log. */
  conn: ServerConnection | null;
  onChange: (change: (m: MachineDraft) => MachineDraft) => void;
}

/** The names and values of an MCP server's env entries or headers. */
function EntryList({
  title,
  entries,
  secretsWritable,
  onChange,
}: {
  title: string;
  entries: McpEntryDraft[];
  secretsWritable: boolean;
  onChange: (entries: McpEntryDraft[]) => void;
}) {
  const patch = (key: number, change: Partial<McpEntryDraft>) => onChange(entries.map((e) => (e.key === key ? { ...e, ...change } : e)));
  return (
    <div className="settings-field settings-field--wide">
      <span className="settings-secret-label">{title}</span>
      {entries.length === 0 && <span className="field-hint">{t("machine.none")}</span>}
      {entries.map((e) => (
        <div key={e.key} className="settings-card">
          <Field label={t("common.name")}>
            <input value={e.name} readOnly={e.secret.saved.set} onChange={(ev) => patch(e.key, { name: ev.target.value })} />
          </Field>
          <SecretField label={t("machine.value")} noun="valor" placeholder={t("machine.valuePlaceholder")} value={e.secret} writable={secretsWritable} onChange={(edit) => patch(e.key, { secret: { ...e.secret, edit } })} />
          {!e.secret.saved.set && (
            <button type="button" className="link-button skills-danger" onClick={() => onChange(entries.filter((x) => x.key !== e.key))}>
              {t("machine.discard")}
            </button>
          )}
        </div>
      ))}
      <button type="button" className="link-button settings-add" onClick={() => onChange([...entries, newEntry()])}>
        {t("machine.addEntry")}
      </button>
    </div>
  );
}

function McpCard({ server, secretsWritable, onChange, onRemove }: { server: McpDraft; secretsWritable: boolean; onChange: (change: Partial<McpDraft>) => void; onRemove: () => void }) {
  return (
    <li className="skills-item settings-card">
      <div className="settings-grid">
        <Field label={t("common.name")}>
          <input value={server.name} onChange={(e) => onChange({ name: e.target.value })} />
        </Field>
        <Field label={t("machine.type")}>
          <select value={server.kind} onChange={(e) => onChange({ kind: e.target.value as McpDraft["kind"] })}>
            <option value="stdio">{t("machine.kindStdio")}</option>
            <option value="http">{t("machine.kindHttp")}</option>
          </select>
        </Field>
        {server.kind === "stdio" ? (
          <>
            <Field label={t("machine.command")} hint={t("machine.commandHint")} wide>
              <input value={server.command} placeholder="npx" onChange={(e) => onChange({ command: e.target.value })} />
            </Field>
            <Field label={t("machine.args")} hint={t("machine.argsHint")} wide>
              <textarea rows={2} value={server.args} onChange={(e) => onChange({ args: e.target.value })} />
            </Field>
            <EntryList title={t("machine.env")} entries={server.env} secretsWritable={secretsWritable} onChange={(env) => onChange({ env })} />
          </>
        ) : (
          <>
            <Field label={t("machine.address")} hint={t("machine.addressHint")} wide>
              <input value={server.url} placeholder={t("machine.urlPlaceholder")} onChange={(e) => onChange({ url: e.target.value })} />
            </Field>
            {server.oauth ? (
              <p className="field-hint settings-field--wide">{t("machine.oauth")}</p>
            ) : (
              <EntryList title={t("machine.headers")} entries={server.headers} secretsWritable={secretsWritable} onChange={(headers) => onChange({ headers })} />
            )}
          </>
        )}
      </div>
      <div className="skills-actions">
        <button type="button" className="link-button skills-danger" onClick={onRemove}>
          {t("machine.removeMcp")}
        </button>
      </div>
    </li>
  );
}

function SshCard({ host, agentIds, onChange, onRemove }: { host: SshDraft; agentIds: string[]; onChange: (change: Partial<SshDraft>) => void; onRemove: () => void }) {
  return (
    <li className="skills-item settings-card">
      <div className="settings-grid">
        <Field label={t("machine.sshName")} hint={t("machine.sshNameHint")}>
          <input value={host.id} onChange={(e) => onChange({ id: e.target.value })} />
        </Field>
        <Field label={t("machine.sshHost")}>
          <input value={host.host} placeholder={t("machine.sshHostPlaceholder")} onChange={(e) => onChange({ host: e.target.value })} />
        </Field>
        <Field label={t("machine.sshUser")}>
          <input value={host.user} onChange={(e) => onChange({ user: e.target.value })} />
        </Field>
        <Field label={t("machine.sshPort")}>
          <input type="number" min={1} max={65535} value={host.port} onChange={(e) => onChange({ port: e.target.value })} />
        </Field>
        <Field label={t("machine.sshKey")} hint={t("machine.sshKeyHint")} wide>
          <input value={host.identityFile} placeholder={t("machine.sshKeyPlaceholder")} onChange={(e) => onChange({ identityFile: e.target.value })} />
        </Field>
      </div>
      <div className="settings-checks">
        <label className="settings-check">
          <input type="checkbox" checked={host.enabled} onChange={(e) => onChange({ enabled: e.target.checked })} />
          {t("machine.sshEnabled")}
        </label>
        <label className="settings-check">
          <input type="checkbox" checked={host.requireApproval} onChange={(e) => onChange({ requireApproval: e.target.checked })} />
          {t("machine.sshApproval")}
        </label>
      </div>
      <div className="settings-field settings-field--wide">
        <span className="settings-secret-label">{t("machine.whoCanUse")}</span>
        <span className="field-hint">{t("machine.whoCanUseHint")}</span>
        <div className="settings-checks">
          {agentIds.map((id) => (
            <label key={id} className="settings-check">
              <input type="checkbox" checked={host.agents.includes(id)} onChange={(e) => onChange({ agents: e.target.checked ? [...host.agents, id] : host.agents.filter((a) => a !== id) })} />
              {id}
            </label>
          ))}
        </div>
      </div>
      <div className="skills-actions">
        <button type="button" className="link-button skills-danger" onClick={onRemove}>
          {t("machine.removeSsh")}
        </button>
      </div>
    </li>
  );
}

export default function MachineSection({ draft, settings, agentIds, secretsWritable, error, conn, onChange }: Props) {
  const writable = settings.writable;
  const set = (change: Partial<MachineDraft>) => onChange((m) => ({ ...m, ...change }));
  const patchMcp = (key: number, change: Partial<McpDraft>) => onChange((m) => ({ ...m, mcp: m.mcp.map((s) => (s.key === key ? { ...s, ...change } : s)) }));
  const patchSsh = (key: number, change: Partial<SshDraft>) => onChange((m) => ({ ...m, ssh: m.ssh.map((h) => (h.key === key ? { ...h, ...change } : h)) }));

  return (
    <Section title={t("machine.title")} hint={t("machine.hint")}>
      <p className="banner settings-danger">{t("machine.danger")}</p>
      {!writable && <p className="banner settings-note">{t("machine.readOnly", { reason: settings.blockedReason })}</p>}
      {error && <p className="error-banner">{error}</p>}

      <fieldset className="settings-machine" disabled={!writable}>
        <div className="settings-checks">
          <label className="settings-check">
            <input type="checkbox" checked={draft.enableShell} onChange={(e) => set({ enableShell: e.target.checked })} />
            {t("machine.shell")}
          </label>
        </div>

        <div className="settings-grid">
          <Field label={t("machine.vaultFolder")} hint={t("machine.vaultFolderHint")} wide>
            <input value={draft.vaultPath} placeholder={t("machine.vaultPlaceholder")} onChange={(e) => set({ vaultPath: e.target.value })} />
          </Field>
          <Field label={t("machine.generatedFolder")} hint={t("machine.generatedHint")} wide>
            <input value={draft.generatedPath} placeholder={t("machine.generatedPlaceholder")} onChange={(e) => set({ generatedPath: e.target.value })} />
          </Field>
        </div>

        <h3 className="settings-subheading">{t("machine.mcpServers")}</h3>
        {draft.mcp.length === 0 && <p className="skills-hint">{t("machine.noMcp")}</p>}
        <ul className="skills-list">
          {draft.mcp.map((s) => (
            <McpCard key={s.key} server={s} secretsWritable={secretsWritable} onChange={(change) => patchMcp(s.key, change)} onRemove={() => onChange((m) => ({ ...m, mcp: m.mcp.filter((x) => x.key !== s.key) }))} />
          ))}
        </ul>
        <button type="button" className="link-button settings-add" onClick={() => onChange((m) => ({ ...m, mcp: [...m.mcp, newMcp()] }))}>
          {t("machine.addMcp")}
        </button>

        <h3 className="settings-subheading">{t("machine.sshServers")}</h3>
        <p className="skills-hint">{t("machine.sshNote")}</p>
        {draft.ssh.length === 0 && <p className="skills-hint">{t("machine.noSsh")}</p>}
        <ul className="skills-list">
          {draft.ssh.map((h) => (
            <SshCard key={h.key} host={h} agentIds={agentIds} onChange={(change) => patchSsh(h.key, change)} onRemove={() => onChange((m) => ({ ...m, ssh: m.ssh.filter((x) => x.key !== h.key) }))} />
          ))}
        </ul>
        <button type="button" className="link-button settings-add" onClick={() => onChange((m) => ({ ...m, ssh: [...m.ssh, newSsh()] }))}>
          {t("machine.addSsh")}
        </button>

        {draft.embedded && (
          <>
            <h3 className="settings-subheading">{t("machine.embeddedTitle")}</h3>
            <p className="skills-hint">{t("machine.embeddedNote")}</p>
            <div className="settings-checks">
              <label className="settings-check">
                <input type="checkbox" checked={draft.embedded.enabled} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, enabled: e.target.checked } }))} />
                {t("machine.startWithDesktop")}
              </label>
              <label className="settings-check">
                <input type="checkbox" checked={draft.embedded.webUi} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, webUi: e.target.checked } }))} />
                {t("machine.serveWeb")}
              </label>
            </div>
            <div className="settings-grid">
              <Field label={t("machine.port")}>
                <input type="number" min={1} max={65535} value={draft.embedded.port} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, port: e.target.value } }))} />
              </Field>
              <Field label={t("machine.hubName")} hint={t("machine.hubNameHint")}>
                <input value={draft.embedded.serverName} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, serverName: e.target.value } }))} />
              </Field>
              <Field label={t("machine.listen")} hint={t("machine.listenHint")} wide>
                <input value={draft.embedded.listenHost} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, listenHost: e.target.value } }))} />
              </Field>
            </div>
            <div className="settings-checks">
              <label className="settings-check">
                <input type="checkbox" checked={draft.embedded.tailscaleCert} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, tailscaleCert: e.target.checked } }))} />
                {t("machine.tailscale")}
              </label>
            </div>
            <div className="settings-grid">
              <Field label={t("machine.ownCert")} hint={t("machine.ownCertHint")}>
                <input value={draft.embedded.tlsCert} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, tlsCert: e.target.value } }))} />
              </Field>
              <Field label={t("machine.privateKey")} hint={t("machine.privateKeyHint")}>
                <input value={draft.embedded.tlsKey} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, tlsKey: e.target.value } }))} />
              </Field>
              <Field label={t("machine.certName")} hint={t("machine.certNameHint")}>
                <input value={draft.embedded.tlsHost} onChange={(e) => onChange((m) => ({ ...m, embedded: m.embedded && { ...m.embedded, tlsHost: e.target.value } }))} />
              </Field>
            </div>
          </>
        )}
      </fieldset>
      {/* Outside the fieldset on purpose: reading the log is not changing the machine, so it works while the settings are locked. */}
      <SshAuditLog conn={conn} />
    </Section>
  );
}
