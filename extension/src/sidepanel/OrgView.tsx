import { useEffect, useState } from "react";
import type { AgentInfo, AgentTask, HubAgents, ModelPolicy, OrgEdit } from "../protocol/messages";
import type { AgentActivityExtrasResponse, AgentTasksResponse, HubAgentsResponse } from "../background/popup_protocol";
import { activityLine, activityOf, lastConversationOf, spendOf } from "./lib/agentTasks";
import { addReportEdit, APPROVAL_CATEGORIES, AUTONOMY_LEVELS, buildOrg, moveEdit, positionEdit, powersEdit, superiorChoices, type OrgNode } from "./lib/org";
import { delegationCandidates, delegationSummary, limitEdit, limitModels, nextPolicyId, policiesEdit, policiesWith, policiesWithout } from "./lib/modelPolicies";
import { t, type Key } from "../i18n/index.ts";
import { rich } from "../i18n/react.tsx";
import PairingKeyForm from "./PairingKeyForm";

const AUTONOMIA: Record<number, Key> = { 1: "org.autonomy.1", 2: "org.autonomy.2", 3: "org.autonomy.3", 4: "org.autonomy.4", 5: "org.autonomy.5" };

/** O que o agente pode fazer, como a tela de Configurações define. */
export function selos(agent: AgentInfo): string[] {
  const lista: string[] = [];
  if (agent.canDelegateToAgents) lista.push(t("org.badge.delegates"));
  if (agent.canManageAgents) lista.push(t("org.badge.manages"));
  if (agent.canMessageAgents) lista.push(t("org.badge.notes"));
  if (agent.canManageTasks) lista.push(t("org.badge.schedules"));
  // Ligadas por padrão: só o que foi tirado aparece.
  if (!agent.canStartTasks) lista.push(t("org.badge.noBackground"));
  if (!agent.canCreateWorkers) lista.push(t("org.badge.noWorkers"));
  if (!agent.canMessageUser) lista.push(t("org.badge.noMessageUser"));
  if (!agent.canChooseModels) lista.push(t("org.badge.noChooseModel"));
  if (agent.autonomy !== 4) lista.push(t("org.badge.autonomy", { level: agent.autonomy, what: AUTONOMIA[agent.autonomy] ? t(AUTONOMIA[agent.autonomy]) : "" }).trim());
  if (agent.approvalRequired.length > 0) lista.push(t(agent.approvalRequired.length > 1 ? "org.badge.asksFirstMany" : "org.badge.asksFirstOne", { count: agent.approvalRequired.length }));
  return lista;
}

/** O que está aberto na árvore: o cargo de um agente, um subordinado novo sob um agente (`null`: no topo) ou a remoção de um. */
type Painel =
  | { kind: "edit"; id: string }
  | { kind: "add"; under: string | null }
  | { kind: "remove"; id: string }
  /** Os modelos que um agente pode escolher ao delegar. */
  | { kind: "limit"; id: string }
  /** O que um agente pode fazer sozinho: autonomia, permissões e tipos de ação que pedem o seu sim (P122). */
  | { kind: "powers"; id: string }
  /** Uma política de modelo: `id` é a que se edita, ou `null` para uma nova. */
  | { kind: "policy"; id: string | null };

interface Edicao {
  onOpenChat: (id: string) => void;
  onOpenTasks: (id: string) => void;
  agents: AgentInfo[];
  /** Provedores, combos e políticas: do que um agente pode ser limitado. */
  candidates: string[];
  /** Provedores e combos: o que uma política pode responder. */
  modelIds: string[];
  policies: ModelPolicy[];
  painel: Painel | null;
  ocupado: boolean;
  abrir: (painel: Painel | null) => void;
  /** Pede a chave de pareamento para a mudança. */
  aplicar: (edit: OrgEdit) => void;
  /** O agente cujo cartão está sendo arrastado (P120): outro cartão, ou o topo, aceita soltá-lo se a mudança é válida. */
  arrastando: string | null;
  arrastar: (id: string | null) => void;
  /** O que o agente andou fazendo, numa linha, tirado das tarefas do hub; `null` sem nenhuma tarefa dele. */
  atividade: (id: string) => string | null;
}

/** Soltar o cartão arrastado sobre `alvo` (`null`: o topo): a mudança, se houver. Um cartão que vira filho de um descendente fecharia um círculo. */
function aoSoltar(edicao: Edicao, alvo: string | null): void {
  const edit = edicao.arrastando === null ? null : moveEdit(edicao.agents, edicao.arrastando, alvo);
  edicao.arrastar(null);
  if (edit) edicao.aplicar(edit);
}

function FormularioCargo({ agent, edicao }: { agent: AgentInfo; edicao: Edicao }) {
  const [cargo, setCargo] = useState(agent.role ?? "");
  const [superior, setSuperior] = useState(agent.reportsTo ?? "");
  const opcoes = superiorChoices(edicao.agents, agent.id);
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.role")}
        <input type="text" value={cargo} maxLength={80} placeholder={t("org.rolePlaceholder")} onChange={(e) => setCargo(e.target.value)} />
      </label>
      <label>
        {t("org.reportsTo")}
        <select value={superior} onChange={(e) => setSuperior(e.target.value)}>
          <option value="">{t("org.nobodyTop")}</option>
          {opcoes.map((a) => (
            <option key={a.id} value={a.id}>
              {a.id}
            </option>
          ))}
        </select>
        <span className="skills-hint">{t("org.reportsHint", { id: agent.id })}</span>
      </label>
      <div className="skills-actions">
        <button type="button" disabled={edicao.ocupado} onClick={() => edicao.aplicar(positionEdit(agent.id, cargo, superior))}>
          {t("common.save")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** O que o agente pode fazer sozinho (P122): o nível de autonomia, as quatro permissões e os tipos de ação que sempre pedem o seu sim. Tudo
 * numa mudança só, com um Salvar, porque cada mudança reconstrói o hub e pede a chave de pareamento. Começa com o que o agente tem hoje. */
function FormularioPoderes({ agent, edicao }: { agent: AgentInfo; edicao: Edicao }) {
  const [autonomia, setAutonomia] = useState(agent.autonomy);
  const [iniciarTarefas, setIniciarTarefas] = useState(agent.canStartTasks);
  const [criarWorkers, setCriarWorkers] = useState(agent.canCreateWorkers);
  const [falarComVoce, setFalarComVoce] = useState(agent.canMessageUser);
  const [escolherModelos, setEscolherModelos] = useState(agent.canChooseModels);
  const [categorias, setCategorias] = useState(() => new Set(agent.approvalRequired));
  // Um id que só um hub mais novo conhece aparece também, pelo próprio id, para não sumir ao salvar.
  const desconhecidas = [...categorias].filter((c) => !APPROVAL_CATEGORIES.some((k) => k.id === c));
  function alternar(id: string) {
    const proximo = new Set(categorias);
    if (proximo.has(id)) proximo.delete(id);
    else proximo.add(id);
    setCategorias(proximo);
  }
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.autonomyOf", { id: agent.id })}
        <select value={autonomia} onChange={(e) => setAutonomia(Number(e.target.value))}>
          {AUTONOMY_LEVELS.map((n) => (
            <option key={n.level} value={n.level}>
              {n.label}
            </option>
          ))}
        </select>
        {autonomia === 5 && !agent.canManageAgents && <span className="skills-hint">{t("org.level5Needs", { id: agent.id })}</span>}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={iniciarTarefas} onChange={(e) => setIniciarTarefas(e.target.checked)} />
        {t("org.canStart")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={criarWorkers} onChange={(e) => setCriarWorkers(e.target.checked)} />
        {t("org.canWorkers")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={falarComVoce} onChange={(e) => setFalarComVoce(e.target.checked)} />
        {t("org.canMessageUser")}
      </label>
      <label className="checkbox-label">
        <input type="checkbox" checked={escolherModelos} onChange={(e) => setEscolherModelos(e.target.checked)} />
        {t("org.canChooseModels")}
      </label>
      <p className="skills-hint">{t("org.asksBefore")}</p>
      {APPROVAL_CATEGORIES.map((c) => (
        <label key={c.id} className="checkbox-label">
          <input type="checkbox" checked={categorias.has(c.id)} onChange={() => alternar(c.id)} />
          {c.label}
        </label>
      ))}
      {desconhecidas.map((id) => (
        <label key={id} className="checkbox-label">
          <input type="checkbox" checked onChange={() => alternar(id)} />
          {id}
        </label>
      ))}
      <div className="skills-actions">
        <button
          type="button"
          disabled={edicao.ocupado}
          onClick={() =>
            edicao.aplicar(
              powersEdit(agent.id, {
                autonomy: autonomia,
                canStartTasks: iniciarTarefas,
                canCreateWorkers: criarWorkers,
                canMessageUser: falarComVoce,
                canChooseModels: escolherModelos,
                approvalRequired: categorias,
              }),
            )
          }
        >
          {t("org.savePowers")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** Os modelos que o agente pode escolher ao delegar (P123): marcados, com um padrão (o primeiro). Nenhum marcado deixa a escolha aberta. */
function FormularioLimite({ agent, edicao }: { agent: AgentInfo; edicao: Edicao }) {
  const [marcados, setMarcados] = useState(() => new Set(agent.delegationModels));
  const [padrao, setPadrao] = useState(agent.delegationModels[0] ?? "");
  const escolhidos = edicao.candidates.filter((id) => marcados.has(id));
  const padraoValido = escolhidos.includes(padrao) ? padrao : (escolhidos[0] ?? "");
  function alternar(id: string) {
    const proximo = new Set(marcados);
    if (proximo.has(id)) proximo.delete(id);
    else proximo.add(id);
    setMarcados(proximo);
  }
  return (
    <div className="connection-form org-form">
      <p className="skills-hint">{t("org.modelsHint", { id: agent.id })}</p>
      {edicao.candidates.length === 0 && <p className="skills-hint">{t("org.noModelsToPick")}</p>}
      {edicao.candidates.map((id) => (
        <label key={id} className="checkbox-label">
          <input type="checkbox" checked={marcados.has(id)} onChange={() => alternar(id)} />
          {id}
          {edicao.policies.some((p) => p.id === id) && <span className="skills-hint">{t("org.policyMark")}</span>}
        </label>
      ))}
      {escolhidos.length > 1 && (
        <label>
          {t("org.defaultModel", { id: agent.id })}
          <select value={padraoValido} onChange={(e) => setPadrao(e.target.value)}>
            {escolhidos.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </label>
      )}
      <div className="skills-actions">
        <button type="button" disabled={edicao.ocupado} onClick={() => edicao.aplicar(limitEdit(agent.id, limitModels(edicao.candidates, marcados, padraoValido)))}>
          {t("common.save")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

/** Uma política de modelo (P123): um nome que o agente que delega pode dizer, o provedor ou combo que responde por ele, e quando escolher. */
function FormularioPolitica({ original, edicao }: { original: ModelPolicy | null; edicao: Edicao }) {
  const [id, setId] = useState(original?.id ?? nextPolicyId(edicao.candidates));
  const [modelo, setModelo] = useState(original?.model ?? edicao.modelIds[0] ?? "");
  const [descricao, setDescricao] = useState(original?.description ?? "");
  const pronto = id.trim() !== "" && modelo !== "";
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.name")}
        <input type="text" value={id} maxLength={64} onChange={(e) => setId(e.target.value)} />
      </label>
      <label>
        {t("org.answers")}
        <select value={modelo} onChange={(e) => setModelo(e.target.value)}>
          {edicao.modelIds.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>
      </label>
      <label>
        {t("org.when")}
        <input type="text" value={descricao} maxLength={200} placeholder={t("org.whenPlaceholder")} onChange={(e) => setDescricao(e.target.value)} />
      </label>
      <div className="skills-actions">
        <button
          type="button"
          disabled={edicao.ocupado || !pronto}
          onClick={() => edicao.aplicar(policiesEdit(policiesWith(edicao.policies, { id, model: modelo, description: descricao }, original?.id ?? null)))}
        >
          {t("org.savePolicy")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

function FormularioNovo({ under, edicao }: { under: string | null; edicao: Edicao }) {
  const [id, setId] = useState("");
  const [cargo, setCargo] = useState("");
  const [persona, setPersona] = useState("");
  const pronto = id.trim() !== "" && persona.trim() !== "";
  return (
    <div className="connection-form org-form">
      <label>
        {t("org.name")}
        <input type="text" value={id} maxLength={64} placeholder={t("org.namePlaceholder")} onChange={(e) => setId(e.target.value)} />
      </label>
      <label>
        {t("org.role")}
        <input type="text" value={cargo} maxLength={80} placeholder={t("org.optional")} onChange={(e) => setCargo(e.target.value)} />
      </label>
      <label>
        {t("org.whatItDoes")}
        <textarea value={persona} rows={3} placeholder={t("org.personaPlaceholder")} onChange={(e) => setPersona(e.target.value)} />
        <span className="skills-hint">
          {under ? t("org.reportsToWho", { who: under }) : t("org.startsAtTop")}
          {t("org.startsCareful")}
        </span>
      </label>
      <div className="skills-actions">
        <button type="button" disabled={edicao.ocupado || !pronto} onClick={() => edicao.aplicar(addReportEdit(id, persona, cargo, under))}>
          {t("org.addAgent")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.cancel")}
        </button>
      </div>
    </div>
  );
}

function ConfirmarRemocao({ node, edicao }: { node: OrgNode<AgentInfo>; edicao: Edicao }) {
  const { agent, children } = node;
  return (
    <div className="connection-form org-form">
      <p className="skills-hint">
        {rich("org.removeQuestion", { id: <strong>{agent.id}</strong> })}
        {children.length > 0 &&
          ` ${t(children.length === 1 ? "org.reportOne" : "org.reportMany", { count: children.length })}${t("org.reportTo", { who: agent.reportsTo ?? t("org.nobodyTopParen") })}`}{" "}
        {t("org.conversationsStay")}
      </p>
      <div className="skills-actions">
        <button type="button" disabled={edicao.ocupado} onClick={() => edicao.aplicar({ kind: "remove", id: agent.id })}>
          {t("common.remove")}
        </button>
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir(null)}>
          {t("common.keep")}
        </button>
      </div>
    </div>
  );
}

function Node({ node, edicao }: { node: OrgNode<AgentInfo>; edicao: Edicao }) {
  const { agent, children } = node;
  const { painel } = edicao;
  const alvoValido = edicao.arrastando !== null && moveEdit(edicao.agents, edicao.arrastando, agent.id) !== null;
  return (
    <li className="org-node">
      {/* O arrastar fica no cartão, não no <li>: o `dragover` dos subordinados não deve subir para o cartão do pai. */}
      <div
        className={`org-card${edicao.arrastando === agent.id ? " org-card--arrastando" : ""}${alvoValido ? " org-card--alvo" : ""}`}
        draggable={!edicao.ocupado}
        onDragStart={(e) => {
          e.dataTransfer.setData("text/plain", agent.id);
          e.dataTransfer.effectAllowed = "move";
          edicao.arrastar(agent.id);
        }}
        onDragEnd={() => edicao.arrastar(null)}
        onDragOver={(e) => {
          if (alvoValido) e.preventDefault();
        }}
        onDrop={(e) => {
          e.preventDefault();
          aoSoltar(edicao, agent.id);
        }}
      >
        <span className="org-name">{agent.id}</span>
        {agent.role && <span className="org-role">{agent.role}</span>}
        <span className="org-badges">
          {selos(agent).map((selo) => (
            <span key={selo} className="org-badge">
              {selo}
            </span>
          ))}
        </span>
        {agent.canDelegateToAgents && <span className="skills-hint org-limit">{delegationSummary(agent.delegationModels)}</span>}
        {edicao.atividade(agent.id) && <span className="skills-hint org-limit">{edicao.atividade(agent.id)}</span>}
        {children.length > 0 && <span className="org-count">{children.length === 1 ? t("org.oneSubordinate") : t("org.nSubordinates", { count: children.length })}</span>}
        <span className="org-actions">
          <button type="button" className="link-button" onClick={() => edicao.onOpenChat(agent.id)} title={t("org.startChat", { id: agent.id })}>
            {t("org.chat")}
          </button>
          <button type="button" className="link-button" onClick={() => edicao.onOpenTasks(agent.id)} title={t("org.tasksOf", { id: agent.id })}>
            {t("org.tasks")}
          </button>
          <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "edit", id: agent.id })}>
            {t("common.edit")}
          </button>
          <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "powers", id: agent.id })} title={t("org.powersOf", { id: agent.id })}>
            {t("org.powers")}
          </button>
          {agent.canDelegateToAgents && (
            <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "limit", id: agent.id })} title={t("org.modelsOf", { id: agent.id })}>
              {t("org.models")}
            </button>
          )}
          <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "add", under: agent.id })}>
            {t("org.addSubordinate")}
          </button>
          <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "remove", id: agent.id })}>
            {t("common.remove")}
          </button>
        </span>
      </div>
      {painel?.kind === "edit" && painel.id === agent.id && <FormularioCargo key={`edit-${agent.id}`} agent={agent} edicao={edicao} />}
      {painel?.kind === "remove" && painel.id === agent.id && <ConfirmarRemocao node={node} edicao={edicao} />}
      {painel?.kind === "powers" && painel.id === agent.id && <FormularioPoderes key={`powers-${agent.id}`} agent={agent} edicao={edicao} />}
      {painel?.kind === "limit" && painel.id === agent.id && <FormularioLimite key={`limit-${agent.id}`} agent={agent} edicao={edicao} />}
      {painel?.kind === "add" && painel.under === agent.id && <FormularioNovo key={`add-${agent.id}`} under={agent.id} edicao={edicao} />}
      {children.length > 0 && (
        <ul className="org-children">
          {children.map((child) => (
            <Node key={child.agent.id} node={child} edicao={edicao} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** P120, P123 — quem reporta a quem entre os agentes, e onde isso se muda: dar um cargo a um agente, passá-lo para baixo de outro, adicionar um
 * subordinado, remover um. Cada mudança é escrita sozinha (nada mais das configurações é tocado) e pede a chave de pareamento do hub. As
 * políticas de modelo e o limite de modelos de cada agente (P123) também se editam aqui, pela mesma operação estreita, sem o formulário inteiro. */
export default function OrgView({ onOpenChat, onOpenTasks }: { onOpenChat: (id: string) => void; onOpenTasks: (id: string) => void }) {
  const [hub, setHub] = useState<HubAgents | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [painel, setPainel] = useState<Painel | null>(null);
  const [asking, setAsking] = useState<OrgEdit | null>(null);
  const [keyError, setKeyError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [arrastando, setArrastando] = useState<string | null>(null);
  const [tasks, setTasks] = useState<AgentTask[]>([]);
  // What each agent spent and when it last had a conversation (P123): extras of the line, like the tasks.
  const [extras, setExtras] = useState<Pick<AgentActivityExtrasResponse, "usage" | "conversations">>({ usage: null, conversations: [] });

  function load() {
    chrome.runtime.sendMessage({ type: "listHubAgents" }).then((res: HubAgentsResponse) => {
      if (res.ok && res.hub) {
        setHub(res.hub);
        setError(null);
      } else {
        setError(res.error ?? t("org.readFailed"));
      }
    });
    // A atividade de cada nó é um extra: sem as tarefas, o cartão só não mostra a linha.
    chrome.runtime.sendMessage({ type: "listAgentTasks" }).then((res: AgentTasksResponse) => {
      if (res.ok) setTasks(res.tasks);
    });
    chrome.runtime.sendMessage({ type: "listAgentActivityExtras" }).then((res: AgentActivityExtrasResponse) => {
      if (res.ok) setExtras({ usage: res.usage, conversations: res.conversations });
    });
  }

  useEffect(load, []);

  function cancelKey() {
    setAsking(null);
    setKeyError(null);
  }

  function confirm(pairingKey: string) {
    if (!asking) return;
    setBusy(true);
    setKeyError(null);
    chrome.runtime.sendMessage({ type: "editAgentOrg", pairingKey, edit: asking }).then((res: HubAgentsResponse) => {
      setBusy(false);
      if (res.ok && res.hub) {
        setHub(res.hub);
        setPainel(null);
        setError(null);
        cancelKey();
      } else if (res.authRejected) {
        setKeyError(t("pairing.wrong"));
      } else {
        cancelKey();
        setError(res.error ?? t("org.changeFailed"));
      }
    });
  }

  if (hub === null) {
    return <div className="agents-pane">{error ? <p className="error-banner">{error}</p> : <p className="skills-hint">{t("common.loading")}</p>}</div>;
  }

  const { agents, modelPolicies, modelIds } = hub;
  const edicao: Edicao = {
    onOpenChat,
    onOpenTasks,
    agents,
    candidates: delegationCandidates(modelIds, modelPolicies),
    modelIds,
    policies: modelPolicies,
    painel,
    ocupado: busy || asking !== null,
    abrir: (next) => {
      setError(null);
      setPainel(next);
    },
    aplicar: (edit) => {
      setError(null);
      setKeyError(null);
      setAsking(edit);
    },
    arrastando,
    arrastar: setArrastando,
    atividade: (id) => {
      const activity = activityOf(tasks, id, { spend: spendOf(extras.usage, id), lastConversationMs: lastConversationOf(extras.conversations, id) });
      return activity ? activityLine(activity, Date.now()) : null;
    },
  };
  const topoValido = arrastando !== null && moveEdit(agents, arrastando, null) !== null;
  const tree = buildOrg(agents);
  const ninguemReporta = tree.every((node) => node.children.length === 0);

  return (
    <div className="agents-pane">
      <div className="skills-toolbar">
        <span className="skills-hint">{t("org.intro")}</span>
        <button type="button" className="link-button" onClick={load} disabled={busy}>
          {t("org.refresh")}
        </button>
      </div>
      {error && <p className="error-banner">{error}</p>}
      {asking && (
        <PairingKeyForm busy={busy} error={keyError} onConfirm={confirm} onCancel={cancelKey}>
          {t("org.pairingHint")}
        </PairingKeyForm>
      )}
      {tree.length === 0 ? (
        <p className="skills-hint">{t("org.noAgents")}</p>
      ) : (
        <ul className="org-tree">
          {tree.map((node) => (
            <Node key={node.agent.id} node={node} edicao={edicao} />
          ))}
        </ul>
      )}
      {topoValido && (
        <div
          className="org-topo"
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault();
            aoSoltar(edicao, null);
          }}
        >
          {t("org.dropHere")}
        </div>
      )}
      {tree.length > 0 && ninguemReporta && <p className="skills-hint">{t("org.nobodyReports")}</p>}
      {painel?.kind === "add" && painel.under === null && <FormularioNovo key="add-top" under={null} edicao={edicao} />}
      <div className="skills-actions">
        <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "add", under: null })}>
          {t("org.addAtTop")}
        </button>
      </div>
      <h2 className="agents-subtitle">{t("org.policies")}</h2>
      {modelPolicies.length === 0 ? (
        <p className="skills-hint">{t("org.noPolicies")}</p>
      ) : (
        <ul className="skills-list">
          {modelPolicies.map((policy) => (
            <li key={policy.id} className="skills-item">
              <span className="skills-item-name">{policy.id}</span> <span className="skills-hint">→ {policy.model}</span>
              {policy.description && <p className="skills-item-description">{policy.description}</p>}
              <span className="org-actions">
                <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.abrir({ kind: "policy", id: policy.id })}>
                  {t("common.edit")}
                </button>
                <button type="button" className="link-button" disabled={edicao.ocupado} onClick={() => edicao.aplicar(policiesEdit(policiesWithout(modelPolicies, policy.id)))}>
                  {t("common.remove")}
                </button>
              </span>
              {painel?.kind === "policy" && painel.id === policy.id && <FormularioPolitica key={`policy-${policy.id}`} original={policy} edicao={edicao} />}
            </li>
          ))}
        </ul>
      )}
      {painel?.kind === "policy" && painel.id === null && <FormularioPolitica key="policy-new" original={null} edicao={edicao} />}
      <div className="skills-actions">
        <button type="button" className="link-button" disabled={edicao.ocupado || modelIds.length === 0} onClick={() => edicao.abrir({ kind: "policy", id: null })}>
          {t("org.newPolicy")}
        </button>
      </div>
    </div>
  );
}
