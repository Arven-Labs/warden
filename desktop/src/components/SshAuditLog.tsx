import { invoke } from "@tauri-apps/api/core";
import { useState } from "react";
import { t } from "../i18n";
import { actionLabel, approvalLabel, detailOf, outcomeOf, wasRefused, whenLabel } from "../lib/sshAudit";
import type { SshAuditEntry } from "../types";

// "Usage log" of the SSH servers (P47): the last calls the SSH tools made on this computer. Read only. Loaded when opened, because the log
// can be big; it never holds a command's output, but the commands themselves can hold a secret.

export default function SshAuditLog() {
  const [entries, setEntries] = useState<SshAuditEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  function load() {
    invoke<SshAuditEntry[]>("list_ssh_audit").then(
      (list) => {
        setEntries(list);
        setError(null);
      },
      (err: unknown) => setError(t("sshLog.readFailed", { error: err instanceof Error ? err.message : String(err) })),
    );
  }

  return (
    <details
      className="ssh-audit"
      onToggle={(e) => {
        if ((e.currentTarget as HTMLDetailsElement).open && entries === null) load();
      }}
    >
      <summary>{t("sshLog.summary")}</summary>
      <p className="settings-hint">{t("sshLog.hint")}</p>
      {error && <p className="settings-error-banner">{error}</p>}
      {entries === null && !error && <p className="settings-hint">{t("common.loading")}</p>}
      {entries !== null && entries.length === 0 && <p className="settings-hint">{t("sshLog.empty")}</p>}
      {entries !== null && entries.length > 0 && (
        <div className="ssh-audit-scroll">
          <table className="usage-table ssh-audit-table">
            <thead>
              <tr>
                <th>{t("sshLog.when")}</th>
                <th>{t("sshLog.server")}</th>
                <th>{t("sshLog.agent")}</th>
                <th>{t("sshLog.action")}</th>
                <th>{t("sshLog.approval")}</th>
                <th>{t("sshLog.what")}</th>
                <th>{t("sshLog.result")}</th>
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
                  <td className="ssh-audit-detail">{detailOf(entry)}</td>
                  <td>{outcomeOf(entry) || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <button type="button" className="settings-browse-btn" onClick={load}>
        {t("common.refresh")}
      </button>
    </details>
  );
}
