# Warden — Estado do Projeto

> Última atualização: 2026-10-17 (Sessão 231)

> Modelo de negócio hospedado (Sessão 220, só desenho, nada implementado): self-host grátis ou assinatura em que o Fabio hospeda o Warden do cliente, com acesso a modelos por créditos pré-pagos, e uma loja gratuita de integrações, skills e agentes prontos, nos dois modos. Ver `HOSPEDAGEM.md` e `PENDING.md` P138 a P148. A seção K do `HOSPEDAGEM.md` (Sessão 222) traz a medição de RAM por instância e a decisão de custo.

> Para o final do projeto (Sessão 217): i18n multi-idioma com inglês como padrão e código em inglês (P136), e a documentação do projeto (P137). Ver `PENDING.md`.

> A spec do router de modelos e da nuvem do Fabio (`SPEC-warden-router-e-futuro.md`) foi removida da raiz na Sessão 221; o conteúdo está no `PENDING.md` P131 e em `HOSPEDAGEM.md` (contexto e preferências), e o arquivo segue no histórico do git (commit `cf00ff7`).

> Ideias registradas sem debate (Sessão 216): modelos de decisão (P132), workspaces isolados (P133), orquestradores de agentes e empresas (P134), SDK de integração (P135). Ver `PENDING.md`.

---

## Como Usar Este Arquivo

O estado do projeto foi dividido em arquivos menores dentro desta pasta (`project/`).
Leia o arquivo relevante para o que você precisa:

| Para saber sobre | Leia |
|---|---|
| Diretrizes de código e ensino | `GUIDELINES.md` |
| Visão geral, stack, status das fases | `OVERVIEW.md` |
| PRD (Product Requirements Document) | `CONTEXT.md` |
| **Todas as fases detalhadas** | **`PHASE.md`** |
| Decisões de arquitetura | `ARCHITECTURE.md` |
| **Pendências (resolvidas e não resolvidas)** | **`PENDING.md`** |
| Roadmap, evoluções planejadas, backlog | `ROADMAP.md` |
| Visão de produto: o Warden como sistema operacional de agentes (hierarquia, modo Agents, Code) | `VISAO_AGENTES.md` |
| Estudos de fora (Hermes e o aprendizado dos agentes, etc.) | `STUDIES.md` |
| **Produto hospedado: assinatura, créditos de modelo e loja (desenho, nada implementado)** | **`HOSPEDAGEM.md`** |
| Log completo de sessões de trabalho | `SESSIONS.md` |

**Ao começar uma sessão**: Diga ao Claude "leia os arquivos em `project/` e me ajude a continuar"
**Ao terminar uma sessão**: Atualize o Log de Sessões em `SESSIONS.md` e marque etapas concluídas. Se resolveu uma pendência, atualize `PENDING.md`.
**Ao tomar uma decisão**: Registre em `ARCHITECTURE.md`
**Ao encontrar um bug/pendência nova**: Adicione em `PENDING.md` com ID sequencial
**Ao mudar de máquina**: Sincronize via git