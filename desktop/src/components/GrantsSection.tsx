import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t, type Key } from "../i18n";
import type { Grant, GrantsView } from "../types";
import { hubGrants, hubRevokeGrant } from "../lib/hub";
import { agoLabel } from "../lib/agentTasks";

const CATEGORIES = new Set(["delete_data", "spend_money", "critical_infra", "external_message", "publish_code", "important_config", "elevated_agent"]);
const categoryLabel = (covers: string) => (CATEGORIES.has(covers) ? t(`grants.${covers}` as Key) : covers);

/** P122 — what the person said "always" to: a risk category or a tool, per conversation and agent. Kept in a file it lapses after the days
 * the host is set to without use; otherwise it holds until the app (or, on a hub, the connection) closes. Taking one back makes the agent
 * ask again; that only makes it more careful, so no pairing key. */
export default function GrantsSection({ remote, titleOf }: { remote: boolean; titleOf: (conversationId: string) => string }) {
  const [view, setView] = useState<GrantsView | null>(null);
  const grants = view?.grants ?? null;
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setView(remote ? await hubGrants() : await invoke<GrantsView>("list_grants"));
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
      setView(
        remote
          ? await hubRevokeGrant(grant)
          : await invoke<GrantsView>("revoke_grant", { conversationId: grant.conversationId, agentId: grant.agentId ?? null, covers: grant.covers }),
      );
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <section className="grants-section">
      <h3 className="settings-title">{t("grants.title")}</h3>
      <p className="settings-hint">
        {t("grants.holdsPer")}{" "}
        {view?.ttlDays != null
          ? view.ttlDays === 1
            ? t("grants.keptOne")
            : t("grants.keptMany", { days: view.ttlDays })
          : remote
            ? t("grants.endsRemote")
            : t("grants.endsLocal")}{" "}
        {t("grants.takeBackHint")}
      </p>
      {error && <p className="usage-error">{error}</p>}
      {grants !== null && grants.length === 0 && <p className="settings-hint">{t("grants.none")}</p>}
      {grants !== null && grants.length > 0 && (
        <ul className="agent-work-list">
          {grants.map((g) => (
            <li key={`${g.conversationId}\u0000${g.agentId ?? ""}\u0000${g.covers}`}>
              <strong>{categoryLabel(g.covers)}</strong> · {g.agentId ?? t("grants.defaultAgent")} · {titleOf(g.conversationId)}
              {g.lastUsedMs != null && ` · ${t("grants.used", { ago: agoLabel(g.lastUsedMs, Date.now()) })}`}{" "}
              <button type="button" className="settings-browse-btn" onClick={() => void revoke(g)}>
                {t("grants.takeBack")}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
