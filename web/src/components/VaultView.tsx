import { useCallback, useEffect, useState, type AnchorHTMLAttributes, type FormEvent } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { VaultConflictError, type ServerConnection, type VaultNote } from "../hub/connection";
import type { VaultSearchHit } from "../hub/messages";
import { t } from "../i18n";

// The hub's vault (P78): a folder tree, word search, and an
// editor. Every save carries the version the note was opened at, so a change the model (or sync,
// or another screen) made meanwhile comes back as a conflict instead of being overwritten.

interface TreeNode {
  name: string;
  path: string;
  isFile: boolean;
  children: TreeNode[];
}

/** Groups `/`-separated paths into a tree, folders first, then alphabetical — same as the desktop's
 * `buildTree`. */
function buildTree(paths: string[]): TreeNode[] {
  const roots: TreeNode[] = [];
  for (const path of paths) {
    const parts = path.split("/");
    let level = roots;
    parts.forEach((part, i) => {
      const isFile = i === parts.length - 1;
      let node = level.find((n) => n.name === part && n.isFile === isFile);
      if (!node) {
        node = { name: part, path: parts.slice(0, i + 1).join("/"), isFile, children: [] };
        level.push(node);
      }
      level = node.children;
    });
  }
  const sort = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => (a.isFile !== b.isFile ? (a.isFile ? 1 : -1) : a.name.localeCompare(b.name, undefined, { sensitivity: "base" })));
    nodes.forEach((n) => sort(n.children));
  };
  sort(roots);
  return roots;
}

/** What a new note's path becomes: trimmed, without leading slashes, `.md` added when it has no
 * extension. */
function normalizeNewPath(input: string): string {
  const path = input.trim().replace(/^\/+/, "");
  const name = path.split("/").pop() ?? "";
  return name.includes(".") ? path : `${path}.md`;
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function ExternalLink({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) {
  return (
    <a {...props} target="_blank" rel="noopener noreferrer">
      {children}
    </a>
  );
}

function VaultTree({ nodes, selected, onOpen }: { nodes: TreeNode[]; selected: string | null; onOpen: (path: string) => void }) {
  return (
    <ul className="vault-tree">
      {nodes.map((node) =>
        node.isFile ? (
          <li key={node.path}>
            <button type="button" className={node.path === selected ? "vault-item vault-item--active" : "vault-item"} onClick={() => onOpen(node.path)}>
              {node.name}
            </button>
          </li>
        ) : (
          <li key={node.path}>
            <details open>
              <summary className="vault-folder">{node.name}</summary>
              <VaultTree nodes={node.children} selected={selected} onOpen={onOpen} />
            </details>
          </li>
        ),
      )}
    </ul>
  );
}

/** The note on the right: loading, failed, or loaded (and maybe being edited). */
type Open =
  | { kind: "none" }
  | { kind: "loading"; path: string }
  | { kind: "error"; path: string; error: string }
  | { kind: "note"; path: string; note: VaultNote; draft: string | null }
  | { kind: "new"; path: string; draft: string };

export default function VaultView({ conn }: { conn: ServerConnection | null }) {
  const [files, setFiles] = useState<string[] | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<VaultSearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [open, setOpen] = useState<Open>({ kind: "none" });
  const [saving, setSaving] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [conflict, setConflict] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listVaultFiles().then(
      (list) => {
        setFiles(list);
        setListError(null);
      },
      (err) => {
        setFiles((current) => current ?? []);
        setListError(t("vault.listError", { error: message(err) }));
      },
    );
  }, [conn]);

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(refresh, [refresh]);

  const dirty = (open.kind === "note" && open.draft !== null && open.draft !== open.note.content) || (open.kind === "new" && open.draft !== "");

  function leaveOk(): boolean {
    return !dirty || window.confirm(t("vault.discard"));
  }

  function resetActionState() {
    setActionError(null);
    setConflict(false);
    setConfirmDelete(false);
  }

  function openFile(path: string, force = false) {
    if (!conn || (!force && !leaveOk())) return;
    resetActionState();
    setOpen({ kind: "loading", path });
    conn.readVaultNote(path).then(
      (note) => setOpen((current) => (current.kind === "loading" && current.path === path ? { kind: "note", path, note, draft: null } : current)),
      (err) => setOpen((current) => (current.kind === "loading" && current.path === path ? { kind: "error", path, error: message(err) } : current)),
    );
  }

  function startNew() {
    if (!leaveOk()) return;
    resetActionState();
    setOpen({ kind: "new", path: "", draft: "" });
  }

  function closeNote() {
    if (!leaveOk()) return;
    resetActionState();
    setOpen({ kind: "none" });
  }

  async function save(overwrite = false) {
    if (!conn || (open.kind !== "note" && open.kind !== "new") || open.draft === null) return;
    const content = open.draft;
    const path = open.kind === "new" ? normalizeNewPath(open.path) : open.path;
    if (open.kind === "new" && path === ".md") {
      setActionError(t("vault.nameIt"));
      return;
    }
    setSaving(true);
    setActionError(null);
    try {
      let expected = open.kind === "note" ? open.note.version : undefined;
      // Overwriting after a conflict: save over whatever is on the hub now.
      if (overwrite) expected = (await conn.readVaultNote(path)).version;
      const version = await conn.saveVaultNote(path, content, expected);
      setConflict(false);
      setOpen({ kind: "note", path, note: { content, version }, draft: null });
      if (open.kind === "new") refresh();
    } catch (err) {
      if (err instanceof VaultConflictError) setConflict(true);
      setActionError(message(err));
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!conn || open.kind !== "note") return;
    setSaving(true);
    setActionError(null);
    try {
      await conn.deleteVaultNote(open.path, open.note.version);
      resetActionState();
      setOpen({ kind: "none" });
      refresh();
    } catch (err) {
      setConfirmDelete(false);
      if (err instanceof VaultConflictError) setConflict(true);
      setActionError(message(err));
    } finally {
      setSaving(false);
    }
  }

  function search(event: FormEvent) {
    event.preventDefault();
    if (!conn) return;
    const text = query.trim();
    if (text === "") {
      setHits(null);
      return;
    }
    setSearching(true);
    conn.searchVault(text).then(
      (found) => {
        setHits(found);
        setSearching(false);
        setListError(null);
      },
      (err) => {
        setSearching(false);
        setListError(t("vault.searchFailed", { error: message(err) }));
      },
    );
  }

  const selectedPath = open.kind === "none" || open.kind === "new" ? null : open.path;
  const isMarkdown = selectedPath?.toLowerCase().endsWith(".md") ?? false;

  return (
    <div className={open.kind === "none" ? "vault-layout" : "vault-layout vault-layout--note-open"}>
      <nav className="vault-nav" aria-label={t("vault.files")}>
        <form className="vault-search" onSubmit={search}>
          <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("vault.search")} aria-label={t("vault.search")} />
        </form>
        <button type="button" className="primary-button" onClick={startNew} disabled={!conn}>
          {t("vault.newNote")}
        </button>
        {listError && <p className="error-banner">{listError}</p>}

        {hits !== null ? (
          <section>
            <div className="vault-section-header">
              <span className="vault-section-label">{searching ? t("vault.searching") : t(hits.length === 1 ? "vault.resultOne" : "vault.resultMany", { count: hits.length })}</span>
              <button
                type="button"
                className="link-button"
                onClick={() => {
                  setHits(null);
                  setQuery("");
                }}
              >
                {t("vault.clear")}
              </button>
            </div>
            {hits.length === 0 && !searching && <p className="skills-hint">{t("vault.nothingFound")}</p>}
            <ul className="vault-tree">
              {hits.map((hit) => (
                <li key={`${hit.path}:${hit.lineNumber}`}>
                  <button type="button" className={hit.path === selectedPath ? "vault-item vault-item--active" : "vault-item"} onClick={() => openFile(hit.path)}>
                    <span className="vault-hit-path">
                      {hit.path}:{hit.lineNumber}
                    </span>
                    <span className="vault-hit-line">{hit.line.trim()}</span>
                  </button>
                </li>
              ))}
            </ul>
          </section>
        ) : (
          <>
            <section>
              <div className="vault-section-label">{t("vault.notes")}</div>
              {files === null ? (
                <p className="skills-hint">{t("vault.loading")}</p>
              ) : files.length === 0 ? (
                <p className="skills-hint">{t("vault.noNotes")}</p>
              ) : (
                <VaultTree nodes={buildTree(files)} selected={selectedPath} onOpen={openFile} />
              )}
            </section>
          </>
        )}
      </nav>

      <div className="vault-pane">
        <div className="chat-header">
          <button type="button" className="link-button vault-back" onClick={closeNote}>
            {t("vault.back")}
          </button>
          <span className="chat-title">{open.kind === "new" ? t("vault.newTitle") : (selectedPath ?? "")}</span>
          {open.kind === "note" && open.draft === null && (
            <span className="vault-actions">
              <button type="button" className="link-button" disabled={!conn} onClick={() => setOpen({ ...open, draft: open.note.content })}>
                {t("vault.edit")}
              </button>
              {confirmDelete ? (
                  <>
                    <button type="button" className="link-button skills-danger" disabled={saving || !conn} onClick={() => void remove()}>
                      {t("vault.deleteForSure")}
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(false)}>
                      {t("vault.keep")}
                    </button>
                  </>
                ) : (
                  <button type="button" className="link-button" onClick={() => setConfirmDelete(true)}>
                    {t("vault.delete")}
                  </button>
                )}
            </span>
          )}
        </div>

        {actionError && (
          <div className="vault-message">
            <p className="error-banner">{actionError}</p>
            {conflict && selectedPath && (
              <div className="skills-actions">
                <button type="button" className="link-button" onClick={() => openFile(selectedPath, true)}>
                  {t("vault.reload")}
                </button>
                {open.kind === "note" && open.draft !== null && (
                  <button type="button" className="link-button skills-danger" disabled={saving} onClick={() => void save(true)}>
                    {t("vault.overwrite")}
                  </button>
                )}
              </div>
            )}
          </div>
        )}

        {open.kind === "none" ? (
          <p className="chat-empty">{t("vault.pick")}</p>
        ) : open.kind === "loading" ? (
          <p className="chat-empty">{t("vault.loading")}</p>
        ) : open.kind === "error" ? (
          <div className="vault-message">
            <p className="error-banner">{open.error}</p>
          </div>
        ) : open.kind === "new" || open.draft !== null ? (
          <form
            className="vault-editor"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            {open.kind === "new" && (
              <label>
                {t("vault.path")}
                <input value={open.path} onChange={(e) => setOpen({ ...open, path: e.target.value })} placeholder={t("vault.pathPlaceholder")} required autoFocus />
                <span className="skills-hint">{t("vault.pathHint")}</span>
              </label>
            )}
            <textarea
              value={open.draft ?? ""}
              onChange={(e) => setOpen({ ...open, draft: e.target.value })}
              placeholder={t("vault.writePlaceholder")}
              aria-label={t("vault.contentLabel")}
              autoFocus={open.kind === "note"}
            />
            <div className="skills-actions">
              <button type="submit" className="primary-button" disabled={saving || !conn}>
                {saving ? t("vault.saving") : t("vault.save")}
              </button>
              <button
                type="button"
                className="link-button"
                disabled={saving}
                onClick={() => {
                  if (!leaveOk()) return;
                  resetActionState();
                  setOpen(open.kind === "note" ? { ...open, draft: null } : { kind: "none" });
                }}
              >
                {t("vault.cancel")}
              </button>
            </div>
          </form>
        ) : (
          <div className="vault-content">
            {open.note.content.trim() === "" ? (
              <p className="skills-hint">{t("vault.empty")}</p>
            ) : isMarkdown ? (
              <div className="bubble-markdown">
                <ReactMarkdown remarkPlugins={[remarkGfm]} components={{ a: ExternalLink }}>
                  {open.note.content}
                </ReactMarkdown>
              </div>
            ) : (
              <pre className="vault-plain">{open.note.content}</pre>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
