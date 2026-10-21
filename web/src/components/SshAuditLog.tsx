import { useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { SshAuditEntry } from "../hub/messages";
import { actionLabel, approvalLabel, detailOf, outcomeOf, whenLabel, wasRefused } from "../hub/sshAudit";
import { t } from "../i18n";

// "SSH usage log" (P47): the latest calls to the SSH tools on this hub. Read-only, owner only. Loads on open, because the log
// can be large, and it does not keep the commands' output, but the commands themselves may hold a secret.

export default function SshAuditLog({ conn }: { conn: ServerConnection | null }) {
  const [entries, setEntries] = useState<SshAuditEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  function load() {
    if (!conn) return;
    conn.listSshAudit().then(
      (list) => {
        setEntries(list);
        setError(null);
      },
      (err: unknown) => setError(t("ssh.loadError", { error: err instanceof Error ? err.message : String(err) })),
    );
  }

  return (
    <details
      className="ssh-audit"
      onToggle={(e) => {
        if ((e.currentTarget as HTMLDetailsElement).open && entries === null) load();
      }}
    >
      <summary>{t("ssh.title")}</summary>
      <p className="skills-hint">{t("ssh.intro")}</p>
      {error && <p className="error-banner">{error}</p>}
      {entries === null && !error && <p className="skills-hint">{t("common.loading")}</p>}
      {entries !== null && entries.length === 0 && <p className="skills-hint">{t("ssh.none")}</p>}
      {entries !== null && entries.length > 0 && (
        <div className="ssh-audit-scroll">
        <table className="usage-table ssh-audit-table">
          <thead>
            <tr>
              <th>{t("ssh.col.when")}</th>
              <th>{t("ssh.col.server")}</th>
              <th>{t("ssh.col.agent")}</th>
              <th>{t("ssh.col.action")}</th>
              <th>{t("ssh.col.approval")}</th>
              <th>{t("ssh.col.what")}</th>
              <th>{t("ssh.col.result")}</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry, index) => (
              <tr key={`${entry.atMs}-${index}`} className={wasRefused(entry) ? "ssh-audit-refused" : undefined}>
                <td>{whenLabel(entry.atMs)}</td>
                <td>{entry.hostId}</td>
                <td>{entry.agent ?? "—"}</td>
                <td>{actionLabel(entry.action)}</td>
                <td>{approvalLabel(entry.approval)}</td>
                <td className="usage-table-key">{detailOf(entry)}</td>
                <td>{outcomeOf(entry) || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
      )}
      <button type="button" className="link-button" onClick={load} disabled={!conn}>
        {t("ssh.refresh")}
      </button>
    </details>
  );
}
