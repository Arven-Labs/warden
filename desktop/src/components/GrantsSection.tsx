import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Grant } from "../types";
import { hubGrants, hubRevokeGrant } from "../lib/hub";
import { agoLabel } from "../lib/agentTasks";

const CATEGORY_LABEL: Record<string, string> = {
  delete_data: "Delete data",
  spend_money: "Spend money",
  critical_infra: "Critical infrastructure",
  external_message: "External message",
  publish_code: "Publish code",
  important_config: "Important configuration",
  elevated_agent: "Agent with elevated permission",
};

/** P122 — what the person said "always" to: a risk category or a tool, per conversation and agent. It holds until the app (or, on a hub,
 * the connection) closes. Taking one back makes the agent ask again; that only makes it more careful, so no pairing key. */
export default function GrantsSection({ remote, titleOf }: { remote: boolean; titleOf: (conversationId: string) => string }) {
  const [grants, setGrants] = useState<Grant[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setGrants(remote ? await hubGrants() : await invoke<Grant[]>("list_grants"));
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, [remote]);

  useEffect(() => {
    void load();
  }, [load]);

  async function revoke(grant: Grant) {
    try {
      setGrants(
        remote
          ? await hubRevokeGrant(grant)
          : await invoke<Grant[]>("revoke_grant", { conversationId: grant.conversationId, agentId: grant.agentId ?? null, covers: grant.covers }),
      );
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className="grants-section">
      <h3 className="settings-title">Permissions given with "always"</h3>
      <p className="settings-hint">
        They hold per conversation and agent. Kept in a file (the default), they survive a restart and lapse after 30 days unused (each use renews
        them); otherwise they end when {remote ? "the connection" : "the app"} closes. Taking one back makes the agent ask again.
      </p>
      {error && <p className="usage-error">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="settings-hint">None yet.</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{CATEGORY_LABEL[g.covers] ?? g.covers}</strong> · {g.agentId ?? "default agent"} · {titleOf(g.conversationId)}
              {g.lastUsedMs != null && ` · used ${agoLabel(g.lastUsedMs, Date.now())}`}{" "}
              <button type="button" className="settings-browse-btn" onClick={() => void revoke(g)}>
                Take back
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
