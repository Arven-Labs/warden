# Warden hospedado: assinatura, créditos de modelo e loja

> Registrado na Sessão 220 (2026-10-15). **É um desenho para debater e implementar depois, em fatias. Nada disto está implementado.** Pendências ligadas: P138 a P141 em `PENDING.md`. Relaciona com P50 (tier pago), P131 (router e nuvem do Fabio), P84 (multiusuário), P85 (marketplace), P133 e P134.

## Para que serve este documento

O Fabio quer que o **mesmo código** funcione de dois jeitos:

1. **Self-host, grátis e completo.** Quem quer hospeda o próprio Warden. Nada básico fica trancado ("o grátis sempre funciona com qualidade padrão; o pago é conveniência ou qualidade extra").
2. **Assinatura.** O Fabio hospeda o Warden inteiro do cliente (hub, agentes, skills, cofre, integrações) nos servidores dele. O cliente só usa.

Dentro da hospedagem, o Fabio também **vende acesso a modelos**: um endpoint de API no servidor dele, no papel de um OpenRouter ou de um NineRouter, só que o cliente pagou a ele e não aos provedores. O cliente escolhe o modelo e usa, sem criar conta em OpenAI, Anthropic ou Google. E há uma **loja** para instalar, com poucos cliques, integrações, skills prontas e **agentes pré-prontos**. O objetivo de produto é tornar tudo o mais fácil possível para o usuário.

**O que é pago e o que não é (esclarecido pelo Fabio na Sessão 220):**

| Gratuito, nos dois modos (self-host e hospedado) | Pago |
|---|---|
| O Warden completo. **A loja**: integrações (plugins), skills e agentes prontos, para fazer integrações de forma simples e ter ferramentas prontas à mão. | **O Fabio hospedar o Warden do cliente**, os **créditos de modelo** da nuvem dele e **alguns outros serviços** (a definir, pergunta 13). |

Ou seja: **a loja não é um produto pago.** Quem hospeda por conta própria tem a loja inteira, de graça, sem conta nem assinatura. Quem é hospedado pelo Fabio tem a mesma loja, só que já ligada à instância dele.

## Decisões do Fabio (2026-10-15)

| # | Decisão | Consequência |
|---|---|---|
| 1 | **Uma instância (contêiner) por cliente** primeiro. O multi-tenant num processo só fica **preparado** para depois. | Funciona com o código de hoje; o custo é RAM por cliente e um orquestrador novo. Os estados globais por processo viram dívida registrada (seção G). |
| 2 | **O plano de controle mora no "site mãe"** da organização: contas, planos, pagamento, tela de provisionamento. | O site mãe não existe neste repositório. Este documento define só o **contrato** (seção B). Sem repositórios novos; crates novos neste repo são aceitos. |
| 3 | **Cobrança só por créditos pré-pagos** no início. Planos (e o abatimento entre plano e crédito) são estudados depois; no fim existirão os dois. | A questão ética do abatimento continua aberta e **não é decidida aqui**. |
| 4 | **Loja gratuita nos dois modos, curada só pelo Fabio** no início. Contém plugins/integrações, skills e agentes pré-prontos. | Terceiros só depois de existirem sandbox e revisão (seção E). A loja precisa funcionar para o self-host sem conta nem login. |

## A. Arquitetura-alvo

```
 ┌──────────────────────── SITE MÃE (externo, não está neste repo) ─────────────────────────┐
 │ contas e login · planos · pagamento · saldo oficial · tela de provisionamento · vitrine   │
 └───────┬───────────────────────────┬─────────────────────────────┬────────────────────────┘
         │ B1 provisionar/suspender  │ B3 créditos e eventos       │ B5 catálogo
         ▼                           ▼                             ▼
 ┌─ ORQUESTRADOR (novo) ─┐   ┌─ GATEWAY DE MODELOS (novo) ─┐   ┌─ LOJA: catálogo + assinatura ─┐
 │ cria, para, atualiza  │   │ /v1 compatível com OpenAI   │   │ pacotes .wpkg assinados        │
 │ contêineres, pastas,  │   │ token por instância (wdh_)  │   └───────────────────────────────┘
 │ domínio, backup,      │   │ checa saldo antes da chamada│
 │ export                │   │ guarda as chaves REAIS dos  │
 └──────────┬────────────┘   │ provedores · ledger durável │
            │                └──────────▲──────────────────┘
            ▼                           │ Provider::Cloud (base_url = gateway,
 ┌─ INSTÂNCIA DO CLIENTE ─────────────┐ │  api_key = token da instância)
 │ 1 contêiner = 1 `warden serve`     │─┘
 │ hub, agentes, skills, cofre, UI    │
 │ HOME / XDG_CONFIG_HOME / XDG_CACHE │
 │ _HOME = /data/<cliente>            │
 │ SEM chave de provedor · SEM saldo  │
 └────────────────────────────────────┘
   (proxy reverso com TLS e domínio por cliente na frente de tudo)
```

**Regra central:** a instância **nunca fala com os provedores e nunca guarda chave de provedor**. Todo modelo pago passa pelo gateway. Isso dá um só lugar para medir, cobrar e cortar abuso, e o cliente nunca vê a chave real.

**Relação com o P131 já feito:** o `Provider::Cloud` e o modo cru da Warden API (Sessão 218) são exatamente as peças que ligam a instância ao gateway. Para o self-hoster, a nuvem é "só mais um provedor". Para o cliente hospedado, é o **único** provedor pago e já vem configurado.

**Crates novos sugeridos (neste repo):** `warden-gateway` (o router hospedado, reaproveitando o núcleo do `openai_api.rs`), `warden-orchestrator`, `warden-store` (pacote, manifesto, assinatura e verificação, usado pelo cliente e pela ferramenta de publicação) e `warden-hosted-protocol` (os tipos do contrato da seção B, para as duas pontas serializarem as mesmas estruturas). A spec lista "criar novos repositórios" como fora de escopo; se o gateway virar produto de fato (multi-tenant, internet, custódia de chaves), a extração é uma decisão futura.

**O que roda onde**

| Por instância (cliente) | Compartilhado, operado pelo Fabio | No site mãe |
|---|---|---|
| `warden serve` completo, config, cofre, agentes, skills, integrações MCP por HTTP, webhooks, um `Provider::Cloud` pré-configurado | gateway de modelos (ledger, cobrança), orquestrador, catálogo e assinatura da loja, proxy reverso (TLS, domínios), backup | identidade, planos, pagamento, saldo "oficial", tela de provisionamento |

## B. Contrato entre o site mãe e o Warden

**Autenticação entre as partes**
- Site mãe -> orquestrador/gateway: mTLS, ou JWT assinado (EdDSA) de vida curta com `aud` e `jti`. O Warden guarda só a chave pública do site mãe. O segredo de provisionamento é separado do de runtime.
- Gateway/orquestrador -> site mãe (webhooks): HMAC sobre o corpo, com timestamp e `Idempotency-Key`.
- Instância -> gateway: **token por instância** (`wdh_…`), revogável, emitido no provisionamento. Não é o login do usuário.

**B1. Provisionamento** (site mãe -> orquestrador, idempotente por `customer_id`)
- `PUT /v1/instances/{customer_id}` com `{plan_ref, region, domain?, limits, owner_identity}`.
- `POST .../{id}:suspend`, `:resume`, `:export`; `DELETE .../{id}` com período de retenção antes de apagar de verdade; `GET .../{id}` devolve estado, versão, saúde e disco.
- O site mãe é dono do estado **comercial**; o orquestrador é dono do estado **operacional** e o reporta de volta.

**B2. Identidade e entitlement**
- O site mãe emite um token de login (OIDC ou JWT). A instância valida pela chave pública, no molde do `truthid_login.rs` (reaproveitar como login federado genérico: a verificar na implementação). O `sub` do site mãe vira o **dono (root)** da instância. A chave de pareamento continua existindo como escape do self-host.
- O estado (`active`, `suspended`, `past_due`) é aplicado pelo **orquestrador** (para ou pausa o contêiner). A instância não se auto-bloqueia, para não pôr lógica de cobrança no código que o self-host também usa.

**B3. Créditos**
- O saldo vive no **ledger do gateway**, em transação atômica: o site mãe é dono do dinheiro e do preço, o gateway é dono do consumo.
- `POST /v1/accounts/{id}/credits` com `{amount_micro_usd, payment_ref}`, idempotente por `payment_ref`, chamado pelo site mãe depois de confirmar o pagamento.
- Webhooks do gateway para o site mãe: `credits.low`, `credits.depleted`, `usage.daily_summary` (e-mail, recarga automática, fatura de consumo).
- `GET /v1/accounts/{id}/balance` e `/usage?from&to` ficam disponíveis para o site mãe e para a tela da instância, só leitura.

**B4. Uso:** o registro detalhado fica no gateway. O site mãe recebe só agregados por dia, modelo e chave.

**B5. Loja:** **gratuita**, sem preço. O site mãe só mostra a vitrine (o self-host usa a mesma loja sem passar por ele). O catálogo técnico (`index.json` assinado) é servido pelo componente da loja (`GET /v1/store/index`, `GET /v1/store/packages/{id}/{versão}`). **O site mãe nunca é a raiz de confiança dos pacotes**: quem assina é a chave de publicação do Fabio.

## C. Créditos e medição

**C1. Ledger durável, separado.** O `spend_ledger.jsonl` é podado (`FileStore::open` com `retain_hours`) e guarda `cost_usd` como `f64`: serve para limites, não para dinheiro. O gateway precisa do seu: SQLite em WAL no começo (Postgres quando houver mais de um gateway), com `accounts`, `credit_entries` (só acrescenta: recarga, débito, estorno, ajuste), `usage_events` e `holds`. Valores em **micro-USD inteiros**.

**C2. Preço de venda x custo.** Hoje o `PriceTable` só aceita `[[prices]]` com o id exato do modelo e não distingue preço de venda. No gateway, um catálogo de modelos guarda `cost_in/out` (o que o provedor cobra) e `sale_in/out` (o que o cliente paga) por milhão de tokens. Cada evento guarda os dois valores, para a margem ser auditável. O cliente só enxerga o cobrado.

**C3. Cobrar antes de cada chamada (reservar, chamar, acertar)**
1. Autenticar o token da instância e carregar a conta.
2. Estimar o pior caso: `entrada estimada x sale_in + max_tokens x sale_out`. Sem `max_tokens` do cliente, o gateway aplica um teto próprio.
3. Criar um `hold` (atualização condicional `saldo - holds >= estimativa`, numa transação só).
4. Chamar o provedor.
5. Acertar com o `usage` real: grava o evento, debita o cobrado, libera o `hold`. Em erro ou corte no meio do stream, cobra o que foi contado (o mesmo contrato do `MeterEvent.ok`).
- Holds expiram por TTL, para o caso de o gateway cair no meio. Um `request_id` único garante que um retry não debita duas vezes.
- Saldo zerado devolve **402** com mensagem clara; a tela da instância mostra "recarregue" com o link do site mãe. **Bloqueia só os modelos pagos, não a instância inteira** (a confirmar, pergunta 3).

**C4. Cotas e abuso**
- Cota por token de instância (gasto por dia e por mês, `max_tokens`, modelos permitidos). Hoje o `Scope` do Warden não tem escopo por chave de API, então no gateway isso nasce nativo.
- Rate limit por token e por IP (token bucket), teto de conexões simultâneas e de streams longos. O `WRONG_KEY_DELAY` por conexão não basta num endpoint público.
- Teto de gasto por hora para conter um token vazado. O **pré-pago** já limita o prejuízo máximo ao saldo pago.
- Pico anormal de uso suspende o token e avisa o site mãe.
- As chaves de API por membro (P84) dentro da instância seguem como estão.

**C5. Caminho de uma chamada hospedada:** agente na instância -> `Provider::Cloud` (`base_url` do gateway) -> o gateway escolhe o provedor real (os combos de roteamento ficam nele) -> resposta em stream. As chaves reais ficam num cofre de segredos do servidor do gateway, **não** no `config.secrets.toml` de nenhuma instância. O cliente escolhe o modelo pelo nome do catálogo.

**Ponto de partida no código:** o `MeterSink` (`warden-core/src/meter.rs`) é a emenda certa, mas **hoje nada o chama**. Ligá-lo (modo `Agent` e modo `Raw`) é pré-requisito. O `LedgerMeter` local continua alimentando limites e telas de uso; no hospedado, o saldo exibido vem do gateway.

## D. Provisionamento e operação (uma instância por cliente)

- **Imagem:** Dockerfile em múltiplos estágios com o binário e a web embutida (**não existe hoje**; o CI só constrói o desktop). Usuário sem privilégios, sistema de arquivos só de leitura exceto `/data`.
- **Por cliente:** volume `/data/<cliente>` com `HOME`, `XDG_CONFIG_HOME` e `XDG_CACHE_HOME` apontando para ele (isso redireciona tudo o que está sob `dirs::config_dir()/warden`; só `config.toml` e o cofre têm flag). `WARDEN_SPEND_LEDGER` explícito. Porta interna fixa; o proxy reverso (Caddy ou Traefik) roteia por subdomínio e faz o ACME, porque o Warden não tem ACME (só `--tailscale-cert` e `--tls-cert`/`--tls-key`). A validar: origem e WebSocket atrás do proxy, e limites diferentes por caminho, já que **uma porta só** serve a UI, o WebSocket, `/v1` e `/hooks`.
- **RAM:** o modelo de embedding (~120 MB, um por processo) é o maior custo fixo, e `warm_embedding_models` roda em todo `serve`. Opções: limite de memória por contêiner com `WARDEN_NO_SEMANTIC=1` no plano básico, ou um serviço de embeddings compartilhado (exige ponto de extensão novo). **Medir o RSS de uma instância ociosa antes de precificar; esse número não existe ainda.** Escalonar o aquecimento em reinícios em massa.
- **Backup:** instantâneo do volume por cliente, cifrado, com retenção definida e teste periódico de restauração. SIGTERM gracioso ou instantâneo do sistema de arquivos para consistência.
- **Upgrades:** imagem versionada, em ondas (canário e depois o resto), com health check (a criar um endpoint de saúde/versão, se não houver) e rollback pela imagem anterior. Formatos de arquivo retrocompatíveis por pelo menos uma versão.
- **Suspender e apagar:** suspender para o contêiner e mantém o volume; apagar só depois da retenção, incluindo as cópias de backup.
- **Sair e voltar (sem prender o cliente):** `:export` gera um pacote versionado da pasta do cliente (config, cofre, agentes, skills, conversas, membros), **sem segredos do gateway**, trocando o `Provider::Cloud` por um espaço para o cliente pôr as próprias chaves. A importação faz o caminho inverso (self-host -> hospedado), com validação de versão. **Nenhum formato de exportação existe hoje** (a verificar o que o `warden-sync` já cobre).
- **Observabilidade:** logs por cliente; RAM, CPU e disco; alarme de contêiner reiniciando.

## E. A loja (gratuita, nos dois modos; fase curada)

**Princípio (esclarecido pelo Fabio, Sessão 220):** a loja **não é paga** e **não é exclusiva do hospedado**. Ela existe para quem faz self-host poder montar integrações de forma simples e ter ferramentas prontas, e também para quem é hospedado. Já estava como ideia no P85 (marketplace de plugins e de skills) e no P76 (catálogo de integrações MCP prontas); esta seção dá a primeira forma concreta. O que a loja oferece:

- **Integrações** (plugins): MCP por HTTP, com os presets de hoje (Filesystem, Google Workspace, Notion, GitHub, Slack, hoje fixos no `SettingsView.tsx`) passando a vir do catálogo.
- **Skills prontas.**
- **Agentes pré-prontos**: persona, autonomia e tools já configurados, instaláveis com um clique (hoje um agente se cria à mão ou pelo gerador).
- **Pacotes combinados** (`bundle`): por exemplo um agente com as skills e a integração de que ele precisa.

**Consequências para o desenho**
- O **catálogo é público e não pede conta nem login**: um self-hoster sem o site mãe baixa o índice assinado e instala. O índice pode ser servido de forma estática (arquivo assinado), sem o gateway nem o orquestrador, e pode ser espelhado.
- A **verificação de assinatura roda no cliente**, que embute a chave pública de publicação. Nada na loja depende de assinatura paga ou de estar hospedado.
- Para o cliente hospedado a diferença é só de conveniência: a instância já vem ligada ao catálogo.
- O que é cobrado continua sendo só a hospedagem, os créditos de modelo e os outros serviços da pergunta 13. A loja entra nos dois lados da tabela do início deste documento, do lado gratuito.

**Pacote `.wpkg`** (tar ou zip) com:
- `manifest.json`: `id`, `version`, `kind` (`skill`, `agent`, `mcp-http`, `bundle`), `title`, `description`, `min_warden_version`, `requires`, `permissions`, `secrets` (campos que o usuário preenche) e `files` com hash SHA-256.
- Conteúdo: uma skill é `skills/<nome>.md` mais `skills/<nome>.files/` (o formato atual de `warden-core/src/skill/mod.rs`); um agente é um trecho de `AgentConfig`; uma integração MCP é um trecho `[[mcp_servers]]` do tipo Http (url, headers, oauth).
- `signature`: Ed25519 sobre o manifesto canônico. O Warden embute as chaves públicas de publicação, com rotação e lista de revogação no índice.

**Instalação na tela**
1. O cliente abre a loja (o catálogo vem com a assinatura do índice verificada).
2. A tela de detalhes mostra o que será criado e as **permissões** (tools que o MCP expõe, domínios acessados, se pede OAuth, qual autonomia as tools recebem).
3. Um clique baixa, verifica assinatura e hashes, grava os arquivos e registra em `installed.json` (id, versão, hash).
4. Segredos e OAuth abrem um formulário ou o fluxo OAuth; o segredo vai para o armazenamento de segredos, nunca para o pacote.
5. Atualizar e desinstalar usam o `installed.json` e removem só o que o pacote criou.

**Segredos hoje:** headers e `env` de MCP ficam em texto puro no `config.toml`, e os tokens OAuth em JSON simples sob `mcp_oauth/`. A loja pede **referência de segredo** (`${secret:nome}`) apontando para o `config.secrets.toml` (0600). É mudança de código (seção G).

**Autonomia:** uma tool MCP sem anotações fica sem classificação e, no nível 4, roda sozinha. O manifesto de um pacote da loja **declara a categoria de risco de cada tool**, e o que não for declarado recebe o padrão mais restritivo.

**Fora de propósito nesta fase:** MCP **stdio** (comando arbitrário no host, sem sandbox; num servidor compartilhado ameaça todos os clientes), terceiros publicando, qualquer pacote com código executável e skills com anexos executáveis (só `.md` e arquivos de dados).

**Para abrir a terceiros depois:** revisão humana e automática com scanner de skills (hoje só existe o `learning::scan_proposal`, restrito ao caminho de proposta do assistente), sandbox para stdio, identidade e chave por publicador, política de retirada e revogação, denúncia, permissões aplicadas em runtime e a decisão central x descentralizada do P85. O ROADMAP já fala em **dois mercados**, o de plugins e o de skills; este desenho começa pelo de skills, agentes e integrações HTTP.

## F. Privacidade e confiança do operador

- **Prometer só o que o modelo atual permite.** O operador consegue, tecnicamente, ver o que o agente vê enquanto trabalha (ARCHITECTURE.md, seção de multiusuário). A cifra por membro (P84) protege backups, discos descartados e leitura casual; **não protege contra quem controla a máquina**. Não prometer "nem nós conseguimos ler".
- **Prometer sim:** acesso restrito e auditado, nenhuma leitura por padrão, nenhum uso dos dados para treino, retenção definida, exportação e apagamento sob demanda.
- **O prompt passa pelo gateway e depois pelo provedor.** Isso entra no texto de privacidade. O gateway registra **só metadados e tokens**, nunca o conteúdo, por padrão.
- **Reforços possíveis:** chave do cofre derivada de uma senha do cliente que não fica em disco (reduz a exposição com a instância parada ou em backup, não enquanto roda destravada). Caminho futuro: o **agente no aparelho do cliente**, com o hub hospedado e a execução sensível no aparelho, pela infraestrutura de nós que já existe (P86/P93).
- **Riscos principais**
  - Token de instância vazado: limitado pelo pré-pago e pelas cotas.
  - Escape de contêiner ou vizinho barulhento: contêiner sem privilégios, cgroups e rede isolada por cliente.
  - **Agente com tools de execução local (`shell`, `code_task`) rodando na máquina do Fabio em nome do cliente:** é o maior risco do hospedado. Pede restrição de saída de rede e revisão do que essas tools podem fazer dentro do contêiner; decidir se começam desligadas (pergunta 8).
  - Chave de provedor do gateway comprometida: cofre de segredos, rotação e orçamento por provedor.
  - Abuso (spam, fraude): limite de envio e rate limit de webhooks.

## G. Lacunas: o que muda e o que é novo

**Mudar no código existente**
- `warden-core/src/meter.rs` e os pontos de chamada em `openai_api.rs` e no caminho de agente: ligar o `MeterSink` em toda chamada de modelo.
- `warden-server/src/openai_api.rs` e `api_keys.rs`: extrair o núcleo do router para o gateway reaproveitar; teto de `max_tokens` e cota por chave (opcionais no self-host).
- `warden-bootstrap/src/secrets.rs` e a leitura de `[[mcp_servers]]`: segredos por referência e tokens OAuth fora de JSON simples.
- Login federado: aceitar um emissor externo configurável, a partir de `truthid_login.rs`.
- Exportar e importar a pasta do cliente (novo comando no `warden-cli`); exportar e importar skills e agentes (não existem em `skill/mod.rs`).
- Endpoint de saúde e versão; opção para desligar ou escalonar `warm_embedding_models` em modo hospedado.
- `desktop/src/components/SettingsView.tsx`: os presets de MCP (hoje fixos no código) passam a vir do catálogo da loja. Telas de saldo, recarga (link), loja e permissões do pacote.

**Novo**
- Os quatro crates da seção A; Dockerfile, compose ou unidades de deploy e CI de imagem; ledger durável, catálogo de preços e cobrança no gateway; ferramenta de publicação e assinatura de pacotes.

**Dívida do multi-tenant** (nada disto muda na fase 1, porque é um processo por cliente; fica anotado como o que bloqueia colocar vários clientes no mesmo processo): `node_model::ROUTER` (um roteador de nós por processo), `member_crypto::KEYRING`, `jobs::task_controls`, `project_scope::SCOPES`, `activity::EVENT_CACHE`, `outreach::SENT`, `message_agent::IN_FLIGHT`, `code_task` LOCKS, `embed::MODEL` (um modelo de embedding por processo, com um só cadeado) e `openai_api::raw_vault`. Variáveis de ambiente também são lidas por processo, então dois hubs no mesmo processo não teriam valores diferentes.

**Fora de escopo agora:** multi-tenant num processo, planos e a compensação plano x crédito, loja de terceiros, sandbox de stdio, P133 (workspaces isolados), P134 (empresas), ACME dentro do Warden.

## H. Fases (cada uma pode sair sozinha quando indicado)

| Fase | Entrega | Depende de | Sai sozinha? |
|---|---|---|---|
| 0 | Fechar as decisões da seção I, medir a RAM de uma instância, ligar o `MeterSink`, corrigir a espera no `Hello` (P142) | nada | sim |
| 1 | Dockerfile, `--data-dir`, endpoint de saúde, tratamento de SIGTERM, log estruturado (P143), formato de exportação e importação | 0 | sim (ajuda o self-host também) |
| 2 | Gateway: token, catálogo de modelos, ledger durável, saldo e `hold`, cotas e rate limit | 0 | sim (testável com a conta criada à mão) |
| 3 | Contrato B (`warden-hosted-protocol`), recarga idempotente, webhooks de saldo | 2 | com um site mãe falso para teste |
| 4 | Orquestrador: provisionar, suspender, apagar, backup, upgrade em ondas | 1 e 3 | só tem uso real com o site mãe |
| 5 | Login federado do site mãe | 3 | em paralelo com a 4 |
| 6 | Loja curada: `warden-store`, telas, segredos por referência, skills e MCP por HTTP | 1 (segredos) | sim, inclusive no self-host |
| 7 | Endurecimento: auditoria de acesso do operador, chave por cliente, teste de restauração | 4 | aos poucos |

**Adiado de propósito:** multi-tenant num processo, planos, terceiros na loja, agente no aparelho do cliente, embeddings compartilhados.

## I. Perguntas ainda abertas (do Fabio)

1. Margem sobre o custo dos provedores: percentual fixo ou por modelo? Há recarga mínima?
2. O saldo expira? Há reembolso do saldo não usado? (afeta contrato, impostos e contabilidade no Brasil)
3. Com saldo zerado, bloqueia só os modelos ou a instância inteira? (recomendação: só os modelos)
4. Quanto tempo reter os dados depois de suspender e depois de cancelar, antes de apagar?
5. Texto de privacidade: prometer só o que o modelo atual permite, ou investir cedo na chave por cliente?
6. Em quais regiões e provedores de infraestrutura hospedar? Há requisito de residência de dados (LGPD)?
7. Quem emite o login no site mãe: OIDC próprio ou o TruthID como emissor?
8. Agentes com tools de execução local no hospedado: liberados, restritos ou desligados no começo?
9. O token da instância é compartilhado com os membros (P84) ou cada membro tem a própria cota?
10. Vender só o gateway de modelos a quem faz self-host (sem hospedar a instância)? O `Provider::Cloud` já permite.
11. Onde fica guardada a chave de publicação da loja e quem pode assinar?
12. Existe uma meta de preço que limite o RAM por instância (decide `WARDEN_NO_SEMANTIC` e embeddings compartilhados)?
13. Quais são os **"outros serviços"** pagos além da hospedagem e dos créditos de modelo? (O Fabio disse que existem; ainda não foram nomeados. Pontos de partida que o ROADMAP já menciona: armazenamento, execução, ferramentas premium, automações e recursos empresariais.)

## J. Revisão de arquitetura do código real (Sessão 220)

Pedida pelo Fabio como um design review de engenheiro sênior para o cenário "instância hospedada por usuário + web UI + desktop conectando na conta", feita lendo o código (sem executar). O que ela mostrou, e que muda ou reforça o desenho acima:

**Correções de premissa**
- O papel servidor ou cliente **não** é uma troca por configuração num app só: são binários diferentes (`warden-server`, e o `desktop`, que **embute** um hub em `desktop/src-tauri/src/server_cmds.rs` e também conecta a hubs remotos em `remote_cmds.rs`).
- O servidor não é Tauri nem usa framework web: HTTP/1.1 escrito à mão sobre `httparse`, WebSocket por `tokio-tungstenite`, TLS por `rustls`, uma porta só (UI, WebSocket, `/v1`, `/hooks`).
- `StorageProvider` e `AuthProvider` **não existem** nem como plano pendente: foram removidos de propósito na Sessão 105 (`SESSIONS.md:2626-2631`). O armazenamento e a autenticação são código concreto (`Vault` preso a um caminho, `PairingStore`, `UserConfig`).

**O que já serve ao cenário**
- O hub **já serve a web UI** embutida no binário (`rust-embed`, `web/dist`, fallback de SPA), e o navegador é "só mais um dispositivo pareado".
- O desktop **já conecta a um hub remoto** (`remote_client.rs`, chave de pareamento, membro ou TruthID, token de dispositivo em `remote_hub.json`).
- Um contêiner por cliente funciona com `HOME`/`XDG_CONFIG_HOME` por cliente.

**Os cinco maiores gaps, em ordem de prioridade**

| # | Gap | Pendência | Fase |
|---|---|---|---|
| 1 | Operar o hub como serviço: imagem, `--data-dir`, health, SIGTERM, log estruturado | P143 | 1 |
| 2 | Identidade por conta: o desktop só conecta com chave de pareamento (32+ caracteres, e que pareia só o primeiro dispositivo), senha de membro ou TruthID; falta um login federado em que o `sub` da conta vira o dono e emite um token de dispositivo | P138 (seção B2) | 5 |
| 3 | Frontend duplicado entre web e desktop, e o desktop remoto preso ao IPC do Tauri | P144 | a debater |
| 4 | Endpoint público sobre HTTP escrito à mão: sem limite por IP, sem teto de conexões, sem CSP nem compressão, sem keep-alive, **sem espera para chave errada no `Hello`** (P142), TLS só por arquivo ou Tailscale. Um proxy reverso (Caddy, Traefik) resolve quase tudo; migrar para hyper ou axum seria grande e não é necessário | P142, P138 | 0 e 1 |
| 5 | Isolamento de execução e de segredos: `shell`, MCP stdio e opencode rodam com o usuário do SO do contêiner, sem sandbox; as chaves de provedor ficam dentro da instância; o operador consegue ver o que o agente vê | P139, P138 | 2 |

**Tradeoffs que ficam em aberto**
- Gap 2: reaproveitar o fluxo do TruthID (menos código, acopla a um emissor) ou OIDC genérico (mais flexível, mais superfície).
- Gap 3: extrair um pacote TypeScript compartilhado com uma interface de transporte (refatoração grande, acaba com a divergência) ou, no desktop remoto, abrir a web UI do hub (barato, mas o webview não tem comandos Tauri, então microfone, cofre local e ferramentas do aparelho não funcionam ali).
- Gap 5: o contêiner por cliente isola um cliente do outro, mas não protege o Fabio do código que o agente executa nem o cliente do Fabio.

**Não verificado nessa revisão:** o comportamento das telas do desktop sem wrapper remoto quando `remote` é verdadeiro; o `warden-cli`, o app móvel e a extensão em profundidade; o fluxo completo de convite de membro e `people::password_gate`; a RAM por instância; um sistema de arquivos só de leitura de ponta a ponta. As contagens de mensagens do protocolo (~98 do cliente, ~80 do servidor) saíram de uma regex e são aproximadas.

## O que foi verificado e o que não foi

Conferido no código em 2026-10-15: `MeterSink` e `LedgerMeter` não têm chamador fora de `meter.rs`; `ApiKey` tem `raw`, `agent_id` e `user`, sem cota; `SpendEvent` guarda tokens de entrada e saída, provedor e combo; `Provider::Cloud` existe; `FileStore::open` poda por `retain_hours`; `WARDEN_SPEND_LEDGER` existe. Vindo da leitura do código por exploração, **não executado**: os caminhos de arquivo sob `dirs::config_dir()`, a lista de estados globais, a ausência de Dockerfile e de ACME, e a ausência de import/export de skills e agentes. **Não medido:** RAM por instância. O `SPEC-lume-desmontagem.md` citado pela spec do router não existe no repositório, e o "site mãe" só aparece na spec e no P131.
