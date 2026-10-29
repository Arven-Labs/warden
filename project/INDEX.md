# Warden — Estado do Projeto

> Última atualização: 2026-10-29 (Sessão 324: P160, a web avisa que há versão nova numa entrada da fonte (PR #10); antes, Sessão 323: P160, o hub lembra de qual curador veio cada skill (PR #9); antes, Sessão 322: os E2E de navegador passam a rodar também em PR (PR #8); antes, Sessão 321: P160, a prévia de um pacote mostra o diff numa skill que a pessoa já tem, PR #7; antes, Sessão 320: P160, o primeiro curador, `community/`; antes, Sessão 319: P160, a seção "Fontes" no desktop; antes, Sessão 318: P160, o índice assinado dos curadores e a seção "Fontes" da web; antes, Sessão 317: P160, criar a chave de assinatura pela web, gerada no navegador; antes, Sessão 316: P131/P140, o E2E do login OAuth dos servidores MCP do hub, no desktop e na web, e o defeito que ele achou (a web não recebia as respostas do login e das chaves confiáveis), CI, Desktop E2E e Web E2E verdes; antes, Sessão 315)

> Modelo de negócio hospedado (Sessão 220, só desenho, nada implementado): self-host grátis ou assinatura em que o Fabio hospeda o Warden do cliente, com acesso a modelos por créditos pré-pagos, e uma loja gratuita de integrações, skills e agentes prontos, nos dois modos. Ver `HOSPEDAGEM.md` e `PENDING.md` P138 a P148. A seção K do `HOSPEDAGEM.md` (Sessão 222) traz a medição de RAM por instância e a decisão de custo.

> Para o final do projeto (Sessão 217): i18n multi-idioma com inglês como padrão e código em inglês (P136, feito até a Sessão 272: as seis superfícies e o código; na 273 o glossário pt-BR, "cofre" e "dispositivo"; na 274 as descrições da galeria de skills em português, o seletor de idioma da web estilizado e a foto do uso do desktop carregada; resta só a conferência do mobile no aparelho), e a documentação do projeto (P137: o primeiro corte está no ar em https://arven-labs.github.io/warden/, vindo da pasta `site/`). Também na Sessão 273: o gateway de modelos com créditos (`crates/warden-gateway`) e o contrato do dinheiro (`crates/warden-hosted-protocol`), P168; na 274 o cartão de créditos chegou ao desktop e ao mobile (na 284 o saldo no cabeçalho do chat e o botão no erro de saldo; na 297 o link de recarga, o campo opcional `topup_url` do hub, sem URL real até o site mãe existir); na 275 o gateway ganhou o rate limit por IP e por token e a suspensão por pico de gasto por hora (na 277 vieram o teto de conexões abertas e de chamadas em andamento por token e o prazo do stream, e na 278 a imagem Docker); na 276 o gateway ganhou TLS próprio (`--tls-cert` e `--tls-key`, com o certificado relido na renovação). Ver `PENDING.md`.

> Loja do P140 (Sessões 257, 296, 300, 301, 302, 303, 304, 305, 309, 310, 311, 312, 313, 314 e 315 (na 315 o desktop passou a fazer esse login com um hub remoto sem colar nada; na 314 o hub passou a fazer o login OAuth dos servidores MCP, com a web como cliente: o Slack, o Notion hospedado e o Sentry entram na galeria da web; na 312 o pacote passou a poder ser assinado pela tela de exportar e de publicar, no desktop e na web; na 311 a prévia da importação ganhou o botão "Confiar nesta chave", no desktop e na web; na 309 o pacote de skill passou a poder ser assinado com Ed25519, com a lista de chaves confiáveis da própria pessoa e a prévia dizendo quem assinou; na 310 essa lista ganhou a tela "Quem assina", no desktop e na web); na 305 a skill da galeria passou a distinguir "editei" de "o catálogo evoluiu", com o estado `outdated` e o botão Atualizar; na 302 as integrações MCP prontas viraram uma galeria no desktop, os 5 de sempre, na 303 uma na web para o hub, só com os 4 locais, e na 304 entraram Linear, GitHub hospedado e Notion hospedado (URL, transporte e login lidos nas docs; ainda não testados nos serviços reais); na 301 a galeria de agentes passou a dizer quais skills do agente a pessoa já tem em outra versão e ficam como estão): a galeria de **skills** (8 embutidas; desktop, web, celular e extensão) e a de **agentes prontos** (4 embutidos: `writer`, `researcher`, `code-reviewer`, `secretary`; hub, web e desktop, na árvore da organização). Na 313 entrou o Sentry hospedado (OAuth, só no desktop; as docs concordam em OAuth e em `/mcp`, e o `Sentry-Bearer` do README não foi oferecido). Faltam outros fornecedores (cada um depois de ler a doc dele) e testar os quatro hospedados nos serviços reais, o pacote `.wpkg` assinado e o Fabio ler os textos. Na 299, uma tool MCP sem anotações passou a valer pelo padrão do protocolo para o agente que exige aprovação (P131).

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