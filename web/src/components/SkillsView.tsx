import { useCallback, useEffect, useState } from "react";
import type { ServerConnection } from "../hub/connection";
import type { CatalogSkillDto, PackagePreviewDto, SkillDto, UserInfo } from "../hub/messages";
import { diffLines, hasChanges, withContext } from "./skillDiff";

// Adapted from the extension's `sidepanel/SkillsView.tsx` (P78), talking to the hub directly
// instead of through a background service worker.

/** What the editor form is doing: `new` (name editable, refuses a taken name) or `edit` (name
 * locked, overwrites) — same split as the desktop's SkillsView. */
interface EditorState {
  mode: "new" | "edit";
  skill: SkillDto;
  /** A suggestion being edited: whether saving accepts it (P104). */
  accept?: boolean;
}

const emptySkill: SkillDto = { name: "", description: "", body: "", agents: [] };

/** As licenças oferecidas ao compartilhar uma skill: o que quem a fez deixa os outros fazerem com ela. */
const LICENSES = [
  { id: "CC0-1.0", label: "CC0 (domínio público, sem condições)" },
  { id: "MIT", label: "MIT (manter o aviso de autoria)" },
  { id: "CC-BY-4.0", label: "CC BY (dar o crédito ao autor)" },
];

/** A skill sendo compartilhada: os dados do arquivo, pedidos antes de gerá-lo. */
interface ExportState {
  name: string;
  version: string;
  author: string;
  license: string;
}

/** Um pacote sendo importado: o texto e, depois de lido e conferido, o que ele diz. */
interface ImportState {
  content: string;
  preview: PackagePreviewDto | null;
  /** A pessoa leu um texto com alerta e quer instalar mesmo assim. */
  allowFlagged: boolean;
  /** A pessoa aceita trocar uma skill dela de mesmo nome que é diferente. */
  replace: boolean;
  /** Um endereço do Arweave digitado para buscar um pacote publicado, em vez de escolher um arquivo. */
  address: string;
}

/** Baixa `content` como um arquivo, pelo navegador. */
function downloadText(fileName: string, content: string) {
  const url = URL.createObjectURL(new Blob([content], { type: "application/json" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

function message(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export default function SkillsView({
  conn,
  user,
  onLearningChange,
}: {
  conn: ServerConnection | null;
  /** Set for a member: their own learning switch shows when the workspace has learning on. */
  user?: UserInfo;
  onLearningChange?: (optOut: boolean) => void;
}) {
  const [skills, setSkills] = useState<SkillDto[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  // A galeria de skills prontas (P140): `null` enquanto carrega ou quando o hub não a tem (um hub anterior à galeria).
  const [catalog, setCatalog] = useState<CatalogSkillDto[] | null>(null);
  const [confirmReplace, setConfirmReplace] = useState<string | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  // Compartilhar uma skill como arquivo e importar uma (P160).
  const [exporting, setExporting] = useState<ExportState | null>(null);
  const [importing, setImporting] = useState<ImportState | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!conn) return;
    conn.listSkills().then(
      (list) => {
        setSkills(list);
        setError(null);
      },
      (err) => {
        setSkills((current) => current ?? []);
        setError(`falha ao listar as skills: ${message(err)}`);
      },
    );
    conn.listSkillCatalog().then(setCatalog, () => setCatalog(null));
  }, [conn]);

  function handleExport() {
    if (!conn || !exporting) return;
    setError(null);
    setNotice(null);
    const author = exporting.author.trim();
    conn.exportSkill(exporting.name, exporting.version.trim(), author === "" ? null : author, exporting.license).then(
      (packed) => {
        downloadText(`${exporting.name}.warden-skill.json`, packed.content);
        setNotice(
          `Arquivo gerado.${packed.attachmentsSkipped ? " Os arquivos anexados a esta skill não vão nele: só o texto é compartilhado." : ""}`,
        );
        setExporting(null);
      },
      (err) => setError(`falha ao gerar o arquivo: ${message(err)}`),
    );
  }

  /** Lê e confere o texto; nada é instalado ainda. */
  function handleCheckImport(content: string) {
    if (!conn) return;
    setError(null);
    conn.previewSkillPackage(content).then(
      (preview) => setImporting({ content, preview, allowFlagged: false, replace: false, address: "" }),
      (err) => {
        // Fica no passo de colar, com o texto, para poder corrigir ou trocar.
        setImporting({ content, preview: null, allowFlagged: false, replace: false, address: "" });
        setError(message(err));
      },
    );
  }

  /** Baixa o pacote publicado no endereço digitado e o lê como se fosse um arquivo. */
  function handleFetchImport() {
    if (!conn || !importing) return;
    setError(null);
    conn.fetchSkillPackage(importing.address.trim()).then(handleCheckImport, (err) => setError(message(err)));
  }

  function handleChooseImportFile(file: File | undefined) {
    if (!file) return;
    file.text().then(handleCheckImport, (err) => setError(`falha ao ler o arquivo: ${message(err)}`));
  }

  function handleInstallImport() {
    if (!conn || !importing?.preview) return;
    setError(null);
    const name = importing.preview.name;
    conn.installSkillPackage(importing.content, importing.replace, importing.allowFlagged).then(
      () => {
        setNotice(`Skill ${name} instalada.`);
        setImporting(null);
        refresh();
      },
      (err) => setError(message(err)),
    );
  }

  function handleInstall(name: string, overwrite: boolean) {
    if (!conn) return;
    setError(null);
    setInstalling(name);
    conn.installCatalogSkill(name, overwrite).then(
      () => {
        setInstalling(null);
        setConfirmReplace(null);
        refresh();
      },
      (err) => {
        setInstalling(null);
        setError(`falha ao instalar a skill: ${message(err)}`);
      },
    );
  }

  // Also re-runs after a reconnect, when `conn` is a new connection.
  useEffect(refresh, [refresh]);

  function handleSave() {
    if (!editor || !conn) return;
    setSaving(true);
    setError(null);
    // Accepting a suggestion is saving it without the mark; editing it without accepting keeps it a suggestion.
    const skill: SkillDto = editor.skill.proposed && editor.accept ? { ...editor.skill, proposed: false } : editor.skill;
    if (!skill.proposed) {
      delete skill.source;
      delete skill.proposedAt;
    }
    conn.saveSkill(skill, editor.mode === "edit").then(
      () => {
        setSaving(false);
        setEditor(null);
        refresh();
      },
      (err) => {
        setSaving(false);
        setError(`falha ao salvar a skill: ${message(err)}`);
      },
    );
  }

  /** Accepts a suggestion as it is. */
  function handleAccept(skill: SkillDto) {
    if (!conn) return;
    setError(null);
    const accepted: SkillDto = { name: skill.name, description: skill.description, body: skill.body, agents: skill.agents };
    conn.saveSkill(accepted, true).then(refresh, (err) => setError(`falha ao aceitar a skill: ${message(err)}`));
  }

  function handleDelete(name: string) {
    if (!conn) return;
    setError(null);
    conn.deleteSkill(name).then(
      () => {
        setConfirmDelete(null);
        refresh();
      },
      (err) => {
        setConfirmDelete(null);
        setError(`falha ao apagar a skill: ${message(err)}`);
      },
    );
  }

  function updateEditor(patch: Partial<SkillDto>) {
    setEditor((current) => (current ? { ...current, skill: { ...current.skill, ...patch } } : current));
  }

  const suggested = (skills ?? []).filter((s) => s.proposed);
  const active = (skills ?? []).filter((s) => !s.proposed);

  if (editor) {
    return (
      <form
        className="skills-editor"
        onSubmit={(e) => {
          e.preventDefault();
          handleSave();
        }}
      >
        <strong>{editor.mode === "new" ? "Nova skill" : `Editar ${editor.skill.name}`}</strong>
        <label>
          Nome
          <input
            value={editor.skill.name}
            onChange={(e) => updateEditor({ name: e.target.value })}
            placeholder="review-pr"
            disabled={editor.mode === "edit"}
            required
          />
          <span className="skills-hint">Minúsculas, números e hífens. Não muda depois de salvar.</span>
        </label>
        <label>
          Descrição
          <input
            value={editor.skill.description}
            onChange={(e) => updateEditor({ description: e.target.value })}
            placeholder="Uma frase: o que faz e quando usar"
            required
          />
        </label>
        <label>
          Instruções
          <textarea
            value={editor.skill.body}
            onChange={(e) => updateEditor({ body: e.target.value })}
            rows={10}
            placeholder="As instruções completas que a IA deve seguir, em markdown."
            required
          />
        </label>
        {editor.skill.agents.length > 0 && (
          <span className="skills-hint">Restrita aos agentes: {editor.skill.agents.join(", ")} (edite no desktop).</span>
        )}
        {editor.skill.proposed && (
          <label className="checkbox-row">
            <input type="checkbox" checked={editor.accept ?? false} onChange={(e) => setEditor({ ...editor, accept: e.target.checked })} />
            Aceitar esta skill: passa a valer nas conversas
          </label>
        )}
        {error && <p className="error-banner">{error}</p>}
        <div className="skills-actions">
          <button type="submit" className="primary-button" disabled={saving || !conn}>
            {saving ? "Salvando…" : "Salvar"}
          </button>
          <button
            type="button"
            className="link-button"
            disabled={saving}
            onClick={() => {
              setEditor(null);
              setError(null);
            }}
          >
            Cancelar
          </button>
        </div>
      </form>
    );
  }

  return (
    <div className="skills-view">
      <div className="skills-toolbar">
        <span className="skills-hint">Instruções que a IA carrega sozinha quando o pedido combina.</span>
        <span className="skills-actions">
          <button
            type="button"
            className="link-button"
            disabled={!conn}
            onClick={() => {
              setError(null);
              setNotice(null);
              setImporting({ content: "", preview: null, allowFlagged: false, replace: false, address: "" });
            }}
          >
            Importar
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={() => {
              setError(null);
              setEditor({ mode: "new", skill: emptySkill });
            }}
          >
            + Nova
          </button>
        </span>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {notice && <p className="skills-hint">{notice}</p>}
      {exporting && (
        <form
          className="skills-editor"
          onSubmit={(e) => {
            e.preventDefault();
            handleExport();
          }}
        >
          <strong>Compartilhar {exporting.name}</strong>
          <span className="skills-hint">
            Gera um arquivo com o texto da skill que qualquer pessoa pode importar. Os arquivos anexados a ela não vão junto. Depois que você
            entrega o arquivo, não dá para desfazer.
          </span>
          <label>
            Versão
            <input value={exporting.version} onChange={(e) => setExporting({ ...exporting, version: e.target.value })} required />
            <span className="skills-hint">Números separados por pontos, como 1.0.0. Aumente quando mudar a skill.</span>
          </label>
          <label>
            Autor (opcional)
            <input value={exporting.author} placeholder="Seu nome ou apelido" onChange={(e) => setExporting({ ...exporting, author: e.target.value })} />
          </label>
          <label>
            Licença
            <select value={exporting.license} onChange={(e) => setExporting({ ...exporting, license: e.target.value })}>
              {LICENSES.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.label}
                </option>
              ))}
            </select>
          </label>
          <div className="skills-actions">
            <button type="submit" className="primary-button" disabled={!conn}>
              Baixar arquivo
            </button>
            <button type="button" className="link-button" onClick={() => setExporting(null)}>
              Cancelar
            </button>
          </div>
        </form>
      )}
      {importing && (
        <div className="skills-editor">
          <strong>Importar uma skill</strong>
          {importing.preview === null ? (
            <>
              <span className="skills-hint">Escolha um arquivo de skill ou cole o texto dele. Nada é instalado até você ler.</span>
              <label>
                Ou um endereço no Arweave
                <input value={importing.address} placeholder="ar://…" onChange={(e) => setImporting({ ...importing, address: e.target.value })} />
              </label>
              <button type="button" className="link-button" disabled={!conn || importing.address.trim() === ""} onClick={handleFetchImport}>
                Buscar
              </button>
              <textarea
                rows={6}
                placeholder="Cole o conteúdo de um arquivo .warden-skill.json"
                value={importing.content}
                onChange={(e) => setImporting({ ...importing, content: e.target.value })}
              />
              <input type="file" accept=".json,application/json" onChange={(e) => handleChooseImportFile(e.target.files?.[0])} />
              <div className="skills-actions">
                <button type="button" className="primary-button" disabled={!conn || importing.content.trim() === ""} onClick={() => handleCheckImport(importing.content)}>
                  Ler
                </button>
                <button type="button" className="link-button" onClick={() => setImporting(null)}>
                  Cancelar
                </button>
              </div>
            </>
          ) : (
            <>
              <span className="skills-item-name">{importing.preview.name}</span>
              <p className="skills-item-description">{importing.preview.description}</p>
              <p className="skills-hint">
                Versão {importing.preview.version} · Licença {importing.preview.license}
                {importing.preview.author ? ` · De ${importing.preview.author}` : " · Sem autor informado"} ·{" "}
                <code>{importing.preview.hash.slice(0, 19)}…</code>
              </p>
              <p className="skills-hint">
                O código acima só mostra que o arquivo não foi alterado no caminho. Ele não prova quem escreveu: leia o texto antes de instalar.
              </p>
              {importing.preview.finding && (
                <p className="error-banner">
                  Este texto contém {importing.preview.finding}. Uma skill são instruções que a IA segue, então confira se é o que você espera.
                </p>
              )}
              <pre className="skills-body">{importing.preview.body}</pre>
              {importing.preview.status === "installed" && <p className="skills-hint">Você já tem exatamente esta skill.</p>}
              {importing.preview.status === "modified" && (
                <label className="checkbox-row">
                  <input type="checkbox" checked={importing.replace} onChange={(e) => setImporting({ ...importing, replace: e.target.checked })} />
                  Trocar a skill que eu já tenho com este nome
                </label>
              )}
              {importing.preview.finding && (
                <label className="checkbox-row">
                  <input type="checkbox" checked={importing.allowFlagged} onChange={(e) => setImporting({ ...importing, allowFlagged: e.target.checked })} />
                  Li e quero instalar mesmo assim
                </label>
              )}
              <div className="skills-actions">
                <button
                  type="button"
                  className="primary-button"
                  disabled={
                    !conn ||
                    importing.preview.status === "installed" ||
                    (importing.preview.status === "modified" && !importing.replace) ||
                    (!!importing.preview.finding && !importing.allowFlagged)
                  }
                  onClick={handleInstallImport}
                >
                  Instalar
                </button>
                <button type="button" className="link-button" onClick={() => setImporting({ ...importing, preview: null })}>
                  Voltar
                </button>
                <button type="button" className="link-button" onClick={() => setImporting(null)}>
                  Cancelar
                </button>
              </div>
            </>
          )}
        </div>
      )}
      {user && user.learningEnabled && (
        <label className="checkbox-row">
          <input
            type="checkbox"
            checked={!user.learningOptOut}
            disabled={!conn}
            onChange={(e) => {
              const optOut = !e.target.checked;
              setError(null);
              conn?.setLearning(!optOut).then(
                () => onLearningChange?.(optOut),
                (err) => setError(`falha ao mudar o aprendizado: ${message(err)}`),
              );
            }}
          />
          A IA pode aprender com minhas conversas e sugerir skills (nada vale até eu aceitar)
        </label>
      )}
      {suggested.length > 0 && (
        <section>
          <h3>Sugeridas pela IA</h3>
          <p className="skills-hint">
            Depois de algumas conversas a IA sugeriu estas skills. <strong>Nada nelas vale até você aceitar</strong>: leia o texto, edite se precisar, e
            aceite ou rejeite.
          </p>
          <ul className="skills-list">
            {suggested.map((skill) => (
              <li key={skill.name} className="skills-item skills-item--suggested">
                <div className="skills-item-header">
                  <span className="skills-item-name">{skill.name}</span>
                  <span className="devices-status devices-status--pending">{skill.revises ? `Alteração de ${skill.revises}` : "Sugerida"}</span>
                </div>
                <p className="skills-item-description">{skill.description || "(sem descrição)"}</p>
                <p className="skills-hint">
                  {skill.source ? <>Da conversa <code>{skill.source}</code></> : "De uma conversa"}
                  {skill.proposedAt ? ` · ${new Date(skill.proposedAt).toLocaleString()}` : ""}
                  {skill.agents.length > 0 ? ` · só para: ${skill.agents.join(", ")}` : ""}
                </p>
                {(() => {
                  const target = skill.revises ? active.find((s) => s.name === skill.revises) : undefined;
                  if (!target) {
                    return (
                      <details>
                        <summary>Ver as instruções</summary>
                        <pre className="skills-body">{skill.body}</pre>
                      </details>
                    );
                  }
                  const diff = diffLines(target.body, skill.body);
                  return (
                    <details>
                      <summary>Ver o que muda em {target.name}</summary>
                      {hasChanges(diff) ? (
                        <pre className="skills-body skills-diff">
                          {withContext(diff).map((line, i) => (
                            <span key={i} className={`skills-diff-line skills-diff-line--${line.op}`}>
                              {line.op === "add" ? "+ " : line.op === "del" ? "- " : "  "}
                              {line.text}
                              {"\n"}
                            </span>
                          ))}
                        </pre>
                      ) : (
                        <p className="skills-hint">Sem mudança no texto das instruções.</p>
                      )}
                    </details>
                  );
                })()}
                {confirmDelete === skill.name ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => handleDelete(skill.name)}>
                      Rejeitar mesmo
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      Manter
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button type="button" className="primary-button" disabled={!conn} onClick={() => handleAccept(skill)}>
                      Aceitar
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor({ mode: "edit", skill, accept: true });
                      }}
                    >
                      Editar
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(skill.name)}>
                      Rejeitar
                    </button>
                  </span>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
      {skills === null ? (
        <p className="skills-hint">Carregando…</p>
      ) : active.length === 0 ? (
        <p className="skills-hint">Nenhuma skill ainda.</p>
      ) : (
        <ul className="skills-list">
          {active.map((skill) => (
            <li key={skill.name} className="skills-item">
              <div className="skills-item-header">
                <span className="skills-item-name">{skill.name}</span>
                {confirmDelete === skill.name ? (
                  <span className="skills-actions">
                    <button type="button" className="link-button skills-danger" onClick={() => handleDelete(skill.name)}>
                      Apagar
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(null)}>
                      Manter
                    </button>
                  </span>
                ) : (
                  <span className="skills-actions">
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setEditor({ mode: "edit", skill });
                      }}
                    >
                      Editar
                    </button>
                    <button
                      type="button"
                      className="link-button"
                      onClick={() => {
                        setError(null);
                        setNotice(null);
                        setExporting({ name: skill.name, version: "1.0.0", author: "", license: LICENSES[0].id });
                      }}
                    >
                      Compartilhar
                    </button>
                    <button type="button" className="link-button" onClick={() => setConfirmDelete(skill.name)}>
                      Apagar
                    </button>
                  </span>
                )}
              </div>
              <p className="skills-item-description">{skill.description || "(sem descrição)"}</p>
              {skill.agents.length > 0 && <p className="skills-hint">Só para: {skill.agents.join(", ")}</p>}
            </li>
          ))}
        </ul>
      )}
      {catalog !== null && catalog.length > 0 && (
        <section>
          <h3>Galeria</h3>
          <p className="skills-hint">
            Skills prontas que vêm com o Warden. Instalar copia a skill para <code>skills/</code> no seu cofre, onde você pode editá-la como
            qualquer outra. Nada é baixado.
          </p>
          <ul className="skills-list">
            {catalog.map((entry) => (
              <li key={entry.name} className="skills-item">
                <div className="skills-item-header">
                  <span className="skills-item-name">{entry.name}</span>
                  {entry.status === "installed" ? (
                    <span className="skills-hint">Instalada</span>
                  ) : entry.status === "modified" && confirmReplace === entry.name ? (
                    <span className="skills-actions">
                      <button
                        type="button"
                        className="link-button skills-danger"
                        disabled={installing === entry.name}
                        onClick={() => handleInstall(entry.name, true)}
                      >
                        Substituir a minha
                      </button>
                      <button type="button" className="link-button" onClick={() => setConfirmReplace(null)}>
                        Manter a minha
                      </button>
                    </span>
                  ) : entry.status === "modified" ? (
                    <span className="skills-actions">
                      <span className="skills-hint">Você tem outra skill com este nome</span>
                      <button type="button" className="link-button" onClick={() => setConfirmReplace(entry.name)}>
                        Usar a da galeria
                      </button>
                    </span>
                  ) : (
                    <span className="skills-actions">
                      <button
                        type="button"
                        className="primary-button"
                        disabled={!conn || installing === entry.name}
                        onClick={() => handleInstall(entry.name, false)}
                      >
                        {installing === entry.name ? "Instalando…" : "Instalar"}
                      </button>
                    </span>
                  )}
                </div>
                <p className="skills-item-description">{entry.description}</p>
                <details>
                  <summary>Ver as instruções</summary>
                  <pre className="skills-body">{entry.body}</pre>
                </details>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
