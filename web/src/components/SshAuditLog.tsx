import { useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { SshAuditEntry } from "../hub/messages";
import { actionLabel, approvalLabel, detailOf, outcomeOf, whenLabel, wasRefused } from "../hub/sshAudit";

// "Registro de uso do SSH" (P47): as últimas chamadas das tools de SSH neste hub. Só leitura, só o dono. Carrega ao abrir, porque o log
// pode ser grande, e não guarda a saída dos comandos, mas os comandos em si podem ter um segredo.

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
      (err: unknown) => setError(`Não foi possível ler o registro: ${err instanceof Error ? err.message : String(err)}`),
    );
  }

  return (
    <details
      className="ssh-audit"
      onToggle={(e) => {
        if ((e.currentTarget as HTMLDetailsElement).open && entries === null) load();
      }}
    >
      <summary>Registro de uso do SSH</summary>
      <p className="skills-hint">
        As últimas chamadas das ferramentas de SSH neste hub, da mais nova à mais antiga. Mostra o comando que o agente pediu (que pode ter um segredo) e como terminou; nunca a saída.
      </p>
      {error && <p className="error-banner">{error}</p>}
      {entries === null && !error && <p className="skills-hint">Carregando…</p>}
      {entries !== null && entries.length === 0 && <p className="skills-hint">Nenhuma chamada registrada ainda.</p>}
      {entries !== null && entries.length > 0 && (
        <div className="ssh-audit-scroll">
        <table className="usage-table ssh-audit-table">
          <thead>
            <tr>
              <th>Quando</th>
              <th>Servidor</th>
              <th>Agente</th>
              <th>Ação</th>
              <th>Aprovação</th>
              <th>O que</th>
              <th>Resultado</th>
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
        Atualizar
      </button>
    </details>
  );
}
