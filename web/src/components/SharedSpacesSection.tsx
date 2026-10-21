import { useCallback, useEffect, useState } from "react";
import { UserError, type ServerConnection } from "../hub/connection";
import type { SpaceInfo, UserInfo } from "../hub/messages";
import { t } from "../i18n";
import { rich } from "../i18n/rich";

// Shared spaces (P84 slice 3): folders of the owner's vault that members see at
// `compartilhado/<name>/` in their own vault — reading only, or writing too. A member's turn never
// sees the rest of the owner's vault. Every change asks for the pairing key, like the people above.

const EVERYONE = "*";

type Access = "none" | "read" | "write";

type Asking =
  /** `originalId` absent: a new space. */
  | { kind: "edit"; originalId?: string; space: SpaceInfo }
  | { kind: "remove"; space: SpaceInfo };

function accessOf(space: SpaceInfo, who: string): Access {
  if (space.writers.includes(who)) return "write";
  if (space.readers.includes(who)) return "read";
  return "none";
}

function withAccess(space: SpaceInfo, who: string, access: Access): SpaceInfo {
  const readers = space.readers.filter((r) => r !== who);
  const writers = space.writers.filter((w) => w !== who);
  if (access === "read") readers.push(who);
  if (access === "write") writers.push(who);
  return { ...space, readers, writers };
}

/** Who is in a space, in a few words. */
function peopleLabel(space: SpaceInfo, users: UserInfo[]): string {
  const name = (id: string) => (id === EVERYONE ? t("spaces.everyone") : (users.find((u) => u.id === id)?.name ?? id));
  const parts = [];
  if (space.writers.length > 0) parts.push(t("spaces.write", { who: space.writers.map(name).join(", ") }));
  if (space.readers.length > 0) parts.push(t("spaces.read", { who: space.readers.map(name).join(", ") }));
  return parts.length > 0 ? parts.join(" · ") : t("spaces.nobody");
}

/** The folders of the owner's vault, from its files, for suggesting one. */
function foldersOf(files: string[]): string[] {
  const folders = new Set<string>();
  for (const file of files) {
    const parts = file.split("/");
    for (let i = 1; i < parts.length; i++) folders.add(parts.slice(0, i).join("/"));
  }
  return [...folders].sort();
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function SharedSpacesSection({ conn, users }: { conn: ServerConnection | null; users: UserInfo[] }) {
  const [spaces, setSpaces] = useState<SpaceInfo[] | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [pairingKey, setPairingKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!conn) return;
    try {
      setSpaces(await conn.listSpaces());
      setError(null);
    } catch (err) {
      setError(message(err));
    }
  }, [conn]);

  useEffect(() => {
    void load();
    conn
      ?.listVaultFiles()
      .then((files) => setFolders(foldersOf(files)))
      .catch(() => setFolders([]));
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
      const space = asking.space;
      setSpaces(
        asking.kind === "edit"
          ? await conn.saveSpace(pairingKey, { ...space, id: space.id.trim().toLowerCase(), folder: space.folder.trim() }, asking.originalId)
          : await conn.deleteSpace(pairingKey, space.id),
      );
      cancel();
    } catch (err) {
      setKeyError(err instanceof UserError && err.authRejected ? t("pairingKey.wrong") : message(err));
    } finally {
      setBusy(false);
    }
  }

  const keyForm = (label: string, danger: boolean, extra: React.ReactNode, ready = true) => (
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
        <span className="field-hint">{t("spaces.pairingKeyHint")}</span>
      </label>
      {keyError && <p className="error-banner">{keyError}</p>}
      <div className="skills-actions">
        <button type="submit" className={danger ? "primary-button skills-danger" : "primary-button"} disabled={busy || !ready || pairingKey.trim() === "" || !conn}>
          {busy ? t("common.wait") : label}
        </button>
        <button type="button" className="link-button" disabled={busy} onClick={cancel}>
          {t("common.cancel")}
        </button>
      </div>
    </form>
  );

  const editor = (editing: Extract<Asking, { kind: "edit" }>) => {
    const space = editing.space;
    const set = (next: SpaceInfo) => setAsking({ ...editing, space: next });
    const everyone = accessOf(space, EVERYONE);
    const accessSelect = (who: string, value: Access) => (
      <select value={value} onChange={(e) => set(withAccess(space, who, e.target.value as Access))}>
        <option value="none">{t("spaces.none")}</option>
        <option value="read">{t("spaces.readOnly")}</option>
        <option value="write">{t("spaces.readWrite")}</option>
      </select>
    );
    return keyForm(
      editing.originalId ? t("spaces.save") : t("spaces.share"),
      false,
      <>
        <label className="settings-field">
          {t("spaces.name")}
          <input autoCapitalize="none" value={space.id} onChange={(e) => set({ ...space, id: e.target.value })} placeholder={t("spaces.namePlaceholder")} autoFocus={!editing.originalId} />
          <span className="field-hint">{t("spaces.nameHint", { path: `compartilhado/${space.id.trim().toLowerCase() || t("spaces.nameFallback")}/` })}</span>
        </label>
        <label className="settings-field">
          {t("spaces.folder")}
          <input autoCapitalize="none" list="shared-space-folders" value={space.folder} onChange={(e) => set({ ...space, folder: e.target.value })} placeholder={t("spaces.namePlaceholder")} />
          <datalist id="shared-space-folders">
            {folders.map((f) => (
              <option key={f} value={f} />
            ))}
          </datalist>
          <span className="field-hint">{t("spaces.folderHint")}</span>
        </label>
        <fieldset className="settings-tools">
          <label className="settings-check">
            {t("spaces.everyoneLabel")} {accessSelect(EVERYONE, everyone)}
          </label>
          {users.map((user) => (
            <label key={user.id} className="settings-check">
              {user.name} {accessSelect(user.id, accessOf(space, user.id))}
            </label>
          ))}
          <span className="field-hint">{t("spaces.onlyYou")}</span>
        </fieldset>
      </>,
      space.id.trim() !== "" && space.folder.trim() !== "",
    );
  };

  return (
    <>
      <div className="skills-header">
        <h2>{t("spaces.title")}</h2>
        <button
          type="button"
          className="primary-button"
          disabled={!conn || asking !== null || users.length === 0}
          onClick={() => setAsking({ kind: "edit", space: { id: "", folder: "", readers: [], writers: [] } })}
        >
          {t("spaces.shareFolder")}
        </button>
      </div>
      <p className="skills-hint">{rich("spaces.intro", { path: <code>compartilhado/</code> })}</p>
      {error && <p className="error-banner">{error}</p>}
      {asking?.kind === "edit" && !asking.originalId && editor(asking)}

      {spaces === null ? (
        <p className="skills-hint">{t("common.loading")}</p>
      ) : spaces.length === 0 ? (
        <p className="skills-hint">{users.length === 0 ? t("spaces.addSomeone") : t("spaces.noneShared")}</p>
      ) : (
        <ul className="skills-list">
          {spaces.map((space) => {
            const mine = asking !== null && (asking.kind === "remove" ? asking.space.id === space.id : asking.originalId === space.id) ? asking : null;
            return (
              <li key={space.id} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">{space.id}</span>
                  <code>{space.folder}</code>
                </div>
                <p className="skills-item-description">{peopleLabel(space, users)}</p>
                {mine?.kind === "edit" && editor(mine)}
                {mine?.kind === "remove" &&
                  keyForm(
                    t("spaces.stopSharing"),
                    true,
                    <p className="error-banner">{t("spaces.stopWarning")}</p>,
                  )}
                {!mine && (
                  <div className="skills-actions">
                    <button type="button" className="link-button" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "edit", originalId: space.id, space })}>
                      {t("common.edit")}
                    </button>
                    <button type="button" className="link-button skills-danger" disabled={!conn || asking !== null} onClick={() => setAsking({ kind: "remove", space })}>
                      {t("spaces.stopSharing")}
                    </button>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
