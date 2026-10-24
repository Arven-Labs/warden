# Warden — Estado do Projeto

> Última atualização: 2026-10-24 (Sessão 287)

> Modelo de negócio hospedado (Sessão 220, só desenho, nada implementado): self-host grátis ou assinatura em que o Fabio hospeda o Warden do cliente, com acesso a modelos por créditos pré-pagos, e uma loja gratuita de integrações, skills e agentes prontos, nos dois modos. Ver `HOSPEDAGEM.md` e `PENDING.md` P138 a P148. A seção K do `HOSPEDAGEM.md` (Sessão 222) traz a medição de RAM por instância e a decisão de custo.

> Para o final do projeto (Sessão 217): i18n multi-idioma com inglês como padrão e código em inglês (P136, feito até a Sessão 272: as seis superfícies e o código; na 273 o glossário pt-BR, "cofre" e "dispositivo"; na 274 as descrições da galeria de skills em português, o seletor de idioma da web estilizado e a foto do uso do desktop carregada; resta só a conferência do mobile no aparelho), e a documentação do projeto (P137: o primeiro corte está no ar em https://arven-labs.github.io/warden/, vindo da pasta `site/`). Também na Sessão 273: o gateway de modelos com créditos (`crates/warden-gateway`) e o contrato do dinheiro (`crates/warden-hosted-protocol`), P168; na 274 o cartão de créditos chegou ao desktop e ao mobile (na 284 o saldo no cabeçalho do chat e o botão no erro de saldo; falta só o link de recarga); na 275 o gateway ganhou o rate limit por IP e por token e a suspensão por pico de gasto por hora (faltam o teto de conexões simultâneas e a imagem Docker); na 276 o gateway ganhou TLS próprio (`--tls-cert` e `--tls-key`, com o certificado relido na renovação). Ver `PENDING.md`.

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
| Verificação por código: fotos das telas da web e do desktop no CI (P161, P165) e as automações ligadas (P162 a P164: RSS semanal, roteiro com modelo real, notas de release, testes que falharam) | `AUTOMACAO.md` |
| Log completo de sessões de trabalho | `SESSIONS.md` |

**Ao começar uma sessão**: Diga ao Claude "leia os arquivos em `project/` e me ajude a continuar"
**Ao terminar uma sessão**: Atualize o Log de Sessões em `SESSIONS.md` e marque etapas concluídas. Se resolveu uma pendência, atualize `PENDING.md`.
**Ao tomar uma decisão**: Registre em `ARCHITECTURE.md`
**Ao encontrar um bug/pendência nova**: Adicione em `PENDING.md` com ID sequencial
**Ao mudar de máquina**: Sincronize via git