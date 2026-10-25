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
| 2 | **O plano de controle mora no "site mãe"** da organização: contas, planos, pagamento, tela de provisionamento. | O site mãe não existe neste repositório ainda. Este documento define só o **contrato** (seção B). Sem repositórios novos; crates novos neste repo são aceitos. **Atualizado na Sessão 272 (direção, nada implementado)**: o site mãe passa a morar neste repositório, numa pasta `site/` (React e TypeScript) que também leva a landing e a documentação (P137); o `web/` segue sendo só o app de cada instância. O dinheiro (ledger, saldo, cobrança) continua nos crates, nunca no front. |
| 3 | **Cobrança só por créditos pré-pagos** no início. Planos (e o abatimento entre plano e crédito) são estudados depois; no fim existirão os dois. | A questão ética do abatimento continua aberta e **não é decidida aqui**. |
| 4 | **Loja gratuita nos dois modos, curada só pelo Fabio** no início. Contém plugins/integrações, skills e agentes pré-prontos. | Terceiros só depois de existirem sandbox e revisão (seção E). A loja precisa funcionar para o self-host sem conta nem login. |

## Contexto e preferências do Fabio que guiam as decisões

Vêm da spec do router (`SPEC-warden-router-e-futuro.md`, de 2026-10-14), que foi removida da raiz na Sessão 221 depois de seu conteúdo ser registrado aqui e no P131 (o arquivo segue no histórico do git, commit `cf00ff7`).

**Contexto.** O Fabio cogitou migrar o roteador de modelos para o Lume, fazendo dele um "centro de APIs". Depois de discutir, a direção mudou: **o Lume será desfeito**, as capacidades financeiras ficam no Anchor e o roteamento de modelos de IA **fica no Warden**. O princípio é um aplicativo central por área (IA no Warden, finanças no Anchor), poucos repositórios e máxima organização. Nenhum projeto dele tem usuários ainda: ele constrói primeiro para uso próprio e quer acertar a arquitetura agora, para não sofrer na hora de escalar. O Warden deve ser cliente das capacidades financeiras do Anchor (a intenção era por MCP; a spec pede para confirmar).

**Preferências que devem guiar as decisões**
- Mínimo de repositórios possível: centralizar por organização, descentralizar só quando necessário.
- Evitar "pontas soltas".
- Arquitetura pensada para o futuro, sem retrabalho para escalar.
- Tradeoffs apresentados antes de implementar.

**Regra de trabalho da spec.** Não presumir nada: explorar o repositório, mostrar ao Fabio o que existe, **confirmar cada decisão de arquitetura** ("vai ser assim mesmo?") e, quando houver mais de um caminho razoável, apresentar opções com prós e contras. O que a spec marca como **[DIREÇÃO]** é ideia a validar, não decisão. Fora de escopo: mexer no TruthID (o Warden não integra além do login opcional já decidido) e criar novos repositórios.

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
- **Decidido na Sessão 273 (a fase 3, lado do dinheiro): HMAC com segredo compartilhado nos dois sentidos**, em vez de mTLS ou JWT EdDSA, por enquanto (sem dependência nova, não exige TLS no gateway, e dá para trocar depois sem mudar os tipos). Cabeçalho `X-Warden-Signature: t=<unix>,v1=<hex>`, com `HMAC-SHA256(segredo, "<t>.<MÉTODO>.<caminho com query>.<corpo>")`; o método e o caminho entram no MAC para que um pedido capturado não valha em outra conta, e a tolerância do relógio é de 300 s. **Dois segredos**: o do site mãe (`--admin-secret`, confere as chamadas dele) e o dos webhooks (`--webhook-secret`, assina o que o gateway manda). Sem o segredo do site mãe as rotas `/v1/accounts/...` nem existem (404). **Rotação (Sessão 285)**: o `--admin-secret-previous` mantém o segredo antigo valendo ao lado do novo até o site passar a assinar com o novo; o dos webhooks troca na ordem site aceita os dois, gateway troca `--webhook-secret`, site tira o antigo. Entrega de webhook **pelo menos uma vez**, com `Idempotency-Key` igual ao id do evento, repetição com espera crescente (30 s, 60 s... no máximo 1 h) e desistência depois de 12 falhas.
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
- Rate limit por token e por IP (token bucket), teto de conexões simultâneas e de streams longos. O `WRONG_KEY_DELAY` por conexão não basta num endpoint público. **Feito na Sessão 275 o rate limit** (`ratelimit.rs`: balde por IP e por token, 600 e 120 por minuto por padrão no `serve`, rajada de um décimo de minuto e no mínimo 5, `429` com `Retry-After`; `/health` e as rotas assinadas do site mãe ficam isentas; atrás do proxy do TLS `--trust-forwarded-for` lê a última entrada do `X-Forwarded-For`, e sem ele o cabeçalho é ignorado). **Feito na Sessão 277 o resto** (`limits.rs`, `Slots`): teto de conexões abertas no total e por IP (tomado no `accept`, antes do TLS; acima do teto a conexão é fechada sem resposta; o teto por IP não vale com `--trust-forwarded-for`), teto de chamadas de chat em andamento por token (`429 rate_limit_exceeded`, antes da reserva) e prazo máximo do stream, com `--max-connections` (1024), `--max-connections-per-ip` (64), `--max-inflight-per-token` (8) e `--max-stream-secs` (600), `0` desliga cada um. O corpo da requisição passou a ser lido **depois** do limite (`read_head` e `read_body`; uma requisição recusada nunca tem o corpo lido).
- Teto de gasto por hora para conter um token vazado. O **pré-pago** já limita o prejuízo máximo ao saldo pago.
- Pico anormal de uso suspende o token e avisa o site mãe. **Feito na Sessão 275**: o pico é o **teto de gasto por hora** (`--hourly-spend-cap-usd`, desligado por padrão): a chamada que leva o cobrado da última hora acima do teto suspende a conta (`active = 0`, a mesma suspensão manual) e enfileira **um** webhook `account.suspended` (`reason: "spend_spike"`, o gasto e o teto). A própria chamada ainda é respondida e cobrada; as seguintes recebem 403 `account_suspended`; retomar é `accounts resume`. Não há coluna de motivo: o motivo vai só no webhook (o esquema não tem migração).
- As chaves de API por membro (P84) dentro da instância seguem como estão.

**C5. Caminho de uma chamada hospedada:** agente na instância -> `Provider::Cloud` (`base_url` do gateway) -> o gateway escolhe o provedor real (os combos de roteamento ficam nele; **feito na Sessão 283 na forma de fallback ordenado por modelo, `[[models.fallback]]` no catálogo; na Sessão 286 o modelo ganhou `strategy` para escolher quem começa: `ordered`, `cheapest`, `round_robin`, `weighted` ou `fastest`**) -> resposta em stream. As chaves reais ficam num cofre de segredos do servidor do gateway, **não** no `config.secrets.toml` de nenhuma instância. (Feito na Sessão 280 na forma enxuta: um arquivo de segredo por provedor, `api_key_file` no catálogo, fora do volume de dados e relido quando muda; sem cofre cifrado por enquanto.) O cliente escolhe o modelo pelo nome do catálogo.

**Ponto de partida no código:** o `MeterSink` (`warden-core/src/meter.rs`) é a emenda certa, mas **hoje nada o chama**. ~~Ligá-lo (modo `Agent` e modo `Raw`) é pré-requisito.~~ **Revisto na Sessão 273: não é pré-requisito, e não foi ligado de propósito.** O gateway mede sozinho, pelo `usage` que o provedor devolve, e o gasto e os limites da instância já são gravados pelo `budget.rs` (`record_in_combo`): ligar o `MeterSink` também contaria cada chamada duas vezes, e só daria uma estimativa que se afasta do saldo real. O `LedgerMeter` local continua alimentando limites e telas de uso; no hospedado, o saldo exibido vem do gateway (`GET /v1/balance`, lido pelo hub e levado na tela de Uso). A emenda só passa a valer se um roteador hospedado precisar medir chamadas próprias.

## D. Provisionamento e operação (uma instância por cliente)

- **Imagem:** Dockerfile em múltiplos estágios com o binário e a web embutida (o hub tem, `Dockerfile`; o gateway também, `Dockerfile.gateway`, desde a Sessão 278). Usuário sem privilégios, sistema de arquivos só de leitura exceto `/data`.
- **Por cliente:** volume `/data/<cliente>` com `HOME`, `XDG_CONFIG_HOME` e `XDG_CACHE_HOME` apontando para ele (isso redireciona tudo o que está sob `dirs::config_dir()/warden`; só `config.toml` e o cofre têm flag). `WARDEN_SPEND_LEDGER` explícito. Porta interna fixa; o proxy reverso (Caddy ou Traefik) roteia por subdomínio e faz o ACME, porque o Warden não tem ACME (só `--tailscale-cert` e `--tls-cert`/`--tls-key`). A validar: origem e WebSocket atrás do proxy, e limites diferentes por caminho, já que **uma porta só** serve a UI, o WebSocket, `/v1` e `/hooks`.
- **RAM (medida na Sessão 222, ver a seção K):** o hub sem os modelos custa **24,8 MB de RSS (3,5 MB privados)**; os dois modelos que o `warm_embedding_models` carrega em todo `serve` somam **~935 MB** (cofre ~135 MB, histórico ~820 MB), e o 1º turno com cofre indexa tudo de uma vez e deixa o RSS em 0,2 a 8 GB. Decidido: modelos **lazy e locais**, indexação em lote de 16, hub sempre de pé e execução pesada em sidecar (seção K.5). O serviço de embeddings compartilhado foi descartado por ora.
- **Backup:** instantâneo do volume por cliente, cifrado, com retenção definida e teste periódico de restauração. SIGTERM gracioso ou instantâneo do sistema de arquivos para consistência. **Implementado na Sessão 233** (`warden-orchestrator backup | backups | restore | verify`, módulo `backup.rs`): o hub é **parado (SIGTERM) durante a cópia** e sobe de novo, também se a cópia falhar (um suspenso fica suspenso); um contêiner descartável (`--network none`, volume `:ro`) roda `tar -czf` sem a pasta `models/` (cache baixável) e o fluxo é cifrado **sem carregar o volume na memória** (AES-256-GCM em blocos de 1 MiB, construção STREAM: chave errada, byte trocado, arquivo cortado e blocos trocados são recusados); a chave é o `backup.key` (0600) na pasta de estado do orquestrador, o mesmo nível de confiança já aceito para as chaves de pareamento; arquivos em `<estado>/backups/<cliente>/<ms>.wbk`, os 7 mais novos por padrão (`--keep`); `backup --all [--every N]` faz todos os ativos. `restore` só entra em volume que **não existe** (trocar dados é `delete --purge-data` antes, de propósito) e apaga o volume pela metade se falhar. `verify` restaura num volume temporário e sobe um hub **sem rede nenhuma e sem tarefas agendadas** (assim o hub restaurado não sincroniza, não chama modelo e não roda nada do cliente), confere `/health` por dentro do contêiner e remove tudo no fim. **Provado no CI** (`scripts/orchestrator-smoke.sh`, passos 8 e restore): arquivo cifrado (o marcador não está em claro), hub de volta, verify bom aceito e adulterado recusado sem sobras, restore e dado de volta. **Fora do host (Sessão 235)**: `--after-backup "<comando>"` (ou `WARDEN_AFTER_BACKUP`, `--after-backup-timeout-secs`, padrão 600) roda um comando do operador depois de cada backup de `backup`, `backup --all` e do `upgrade`: `rclone`, `scp`, `rsync`, `aws s3 cp`, o que ele quiser; o orquestrador não fala com nuvem nenhuma. O texto é separado em argumentos **antes** de qualquer substituição e **sem shell**; `{file}`, `{customer}` e `{name}` são trocados dentro de cada argumento (um caminho com espaço nunca vira comando), e `WARDEN_BACKUP_FILE` e `WARDEN_BACKUP_CUSTOMER` vão no ambiente. Falha (código ≠ 0, programa que não existe, estouro do tempo, em que o processo é morto) vira erro que diz onde o backup ficou (`the backup is at <arquivo>, but it could not be shipped off the host`) e **o arquivo local fica**; o erro cita o programa e as últimas linhas de stderr, nunca os argumentos (podem ter credencial). No `upgrade`, um envio que falha é só aviso: a cópia local já é a volta do cliente. O arquivo já sai cifrado; a **chave** não vai pelo mesmo caminho (cofre e chave juntos): `warden-orchestrator backup-key` a imprime (64 hex, o conteúdo do `backup.key`), e num host novo ela volta para `<estado>/backup.key` (0600) com o `.wbk` baixado pela ferramenta do operador, que o `restore` e o `verify` aceitam por caminho. **Provado no CI**: a fumaça confere a cópia idêntica (`cmp`) e aceita pelo `verify`, e um comando que falha faz o backup sair com erro, com o hub de volta e o arquivo local no lugar. **Não cobre**: podar o destino (a ferramenta ou a política do bucket do operador), baixar de volta dentro do `restore`, destino nativo S3 ou SSH, a chave num cofre de segredos, e conferir o arquivo depois de enviado (o operador roda o `verify` na cópia). **Também não cobre**: apagar os backups junto com `--purge-data` e o prazo de retenção antes de apagar (pergunta 4), agendamento como serviço, backup incremental, e o RSS/tempo de parada com um cofre grande real (a parada dura o tempo do `tar`).
- **Upgrades:** imagem versionada, em ondas (canário e depois o resto), com health check (a criar um endpoint de saúde/versão, se não houver) e rollback pela imagem anterior. Formatos de arquivo retrocompatíveis por pelo menos uma versão. **Implementado na Sessão 234** (`warden-orchestrator upgrade --image I`, módulo `upgrade.rs` e `InstanceManager::upgrade`): cada instância recebe um **backup antes** (`--no-backup` pula), o contêiner é parado, removido e criado de novo da imagem nova **no mesmo volume**, e o hub tem de responder `/health` (consultado por dentro do contêiner, `--health-timeout-secs`, padrão 60) e continuar respondendo depois de um período de observação (`--soak-secs`, padrão 30). Se não fica saudável, **a imagem antiga volta** (mesma checagem, sem a observação) e o resultado é `rolled back`, com o caminho do backup; se a antiga também não sobe, `DOWN`, com o backup como saída. Ordem: o **canário** (`--canary`, padrão 1) sozinho, depois ondas de `--wave-size` (padrão 5) em paralelo; **o primeiro rollback pára as ondas que não começaram** (`skipped`), e o comando sai com erro, então rodar de novo depois do conserto continua de onde parou (quem já está na imagem fica fora). Uma instância suspensa só tem a imagem gravada e o contêiner parado removido; o `resume` sobe na imagem nova. **Provado no CI** (`scripts/orchestrator-smoke.sh`, passo 9): upgrade para a imagem nova no mesmo volume com backup antes, repetir não faz nada, e uma imagem que nunca fica saudável volta à anterior com o hub de pé e o dado intacto. **Não cobre**: voltar os **dados** (só a imagem volta; se a versão nova migrou o formato, o `restore` do backup é manual), escolher clientes (`--customer`), janela de manutenção e agendamento, imagem por plano, e a observação é só `/health` (não olha erro nos logs nem `/status`).
- **Suspender e apagar:** suspender para o contêiner e mantém o volume; apagar só depois da retenção, incluindo as cópias de backup.
- **Sair e voltar (sem prender o cliente):** `:export` gera um pacote versionado da pasta do cliente (config, cofre, agentes, skills, conversas, membros), **sem segredos do gateway**, trocando o `Provider::Cloud` por um espaço para o cliente pôr as próprias chaves. A importação faz o caminho inverso (self-host -> hospedado), com validação de versão. **Implementado na Sessão 225** como `warden-server export` e `warden-server import` (P141): pacote em **pasta** com manifesto versionado, lista de inclusão fechada, chaves dos provedores só com `--with-secrets` e o provedor `Cloud` sempre sem chave. O `warden-sync` cobria só cofre, config e logins MCP, então não serviu de base.
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

**Estado (Sessão 257):** feita a 1ª fatia só de **skills**, sem pacote nem assinatura: 8 skills embutidas no binário (`warden-core/catalog/skills/`), instaladas com um clique no hub, no desktop e na web, com o estado deduzido do cofre (sem `installed.json`). O pacote `.wpkg`, a assinatura e o índice seguem desenhados e não feitos. **Sessão 300 e 301**: os agentes prontos (hub, web e desktop). **Sessão 302**: as integrações MCP prontas, **só no desktop** e com os 5 de sempre (`desktop/src/lib/mcpCatalog.ts`; inclui as locais por npx e Docker, que o desenho acima deixa fora do catálogo compartilhado); **Sessão 303**: a mesma galeria na web, para o hub (seção "Máquina"), só com os 4 locais, sem o Slack (OAuth só no desktop); uma integração HTTP no hub depende de endpoint verificado na doc do fornecedor. **Sessão 304**: entraram as primeiras por HTTP, verificadas nas docs (2026-10-25): Linear e GitHub hospedado (chave em `Authorization: Bearer`, no desktop e na web) e Notion hospedado (OAuth, só desktop). O Sentry ficou fora porque as duas páginas oficiais divergem sobre o login. Falta testá-las contra os serviços reais. **Sessão 305**: a skill da galeria passou a distinguir "editei" de "o catálogo evoluiu" (estado `outdated`, por um registro local `skills/.catalog.json`, sem `installed.json` compartilhado); um pacote de fora nunca ganha a atualização sem confirmação.

**Fora de propósito nesta fase:** MCP **stdio** (comando arbitrário no host, sem sandbox; num servidor compartilhado ameaça todos os clientes), terceiros publicando, qualquer pacote com código executável e skills com anexos executáveis (só `.md` e arquivos de dados).

**Para abrir a terceiros depois:** revisão humana e automática com scanner de skills (hoje só existe o `learning::scan_proposal`, restrito ao caminho de proposta do assistente), sandbox para stdio, identidade e chave por publicador, política de retirada e revogação, denúncia, permissões aplicadas em runtime e a decisão central x descentralizada do P85. O ROADMAP já fala em **dois mercados**, o de plugins e o de skills; este desenho começa pelo de skills, agentes e integrações HTTP.

### E.2 Skills da comunidade, descentralizadas (proposta da Sessão 257, a confirmar; P160)

O usuário quer que a comunidade crie e publique skills, que o app venha **sem nenhuma fixa no código** e que o Fabio publique as que usa. Prefere algo descentralizado; o servidor com contas, upload e avaliações fica **para quando virar produto** (será mais um curador, ver abaixo). Desenho proposto, três camadas independentes:

- **Pacote**: o texto da skill (formato do cofre) mais um `manifest` com `id`, `version`, `author`, `license`, `kind`. Identificado pelo hash SHA-256 do conteúdo e **assinado pela chave Ed25519 do autor**: a identidade é a chave, não uma conta em algum serviço. A reaproveitar: o TruthID do projeto, a conferir.
- **Transporte**: onde os bytes ficam não importa para a confiança. https ou git no início; Arweave, IPFS ou Nostr podem entrar depois como outros transportes, porque o cliente só precisa dos bytes e confere o hash.
- **Curadores**: qualquer pessoa publica um **índice assinado** (hashes, título, descrição, autor, e uma lista de revogados). O usuário **assina os curadores em que confia**, por URL ou chave pública, nas configurações. O Warden não traz nenhum; a tela de skills abre vazia com um botão opcional "adicionar a fonte do Fabio". Um pacote fora de todo curador só instala por hash ou URL direta, com aviso. Revogar é o curador tirar do índice e listar nos revogados, ou o autor assinar a revogação da própria chave.

**Primeiro curador**: um repositório público do Fabio no GitHub (custo zero; pull request como fluxo de publicação; CI roda o scanner e valida o formato; os mantenedores revisam). **No produto hospedado**: um servidor com contas, upload na interface e avaliações, que é só mais um curador na mesma lista.

**Segurança**: uma skill é texto que o modelo lê; o risco é prompt injection (mandar vazar dados, esconder ações, ignorar regras), não código. Instalar mostra o texto inteiro e roda o `learning::scan_proposal` (links, comandos, chaves, texto invisível); atualizar mostra o diff e nunca é automático; as aprovações e o nível de autonomia do agente continuam valendo, então uma skill não dá mais poder do que a configuração permite. Fora da v1: skills com anexos executáveis (pedem sandbox).

**Comparação das saídas descentralizadas**: *Nostr* (eventos assinados em relays, sem servidor nosso, identidade = chave) encaixa bem, mas depende de relays que podem descartar dados e não resolve spam sozinho; *Arweave* (o sync já usa) dá permanência, mas não se apaga, então a revogação vive só na lista; *IPFS* é endereçado por conteúdo mas precisa de quem mantenha o pin. Por isso o formato não depende de nenhum: começa por https e entra o resto quando houver comunidade.

**Biblioteca hospedada (pedido da Sessão 258, para quando virar produto)**: o curador do produto lista as skills achadas no Arweave e aceita **pedidos de publicação** de quem não quer usar o TruthID. O fluxo: a pessoa exporta o pacote (ou escreve na interface), aceita a licença e envia; o pedido entra numa fila com a varredura e a revisão; se for aprovado, o Fabio publica pelo `pin()` do TruthID dele e **paga a taxa**, e o `ar://` entra no índice assinado da biblioteca. Decisões em aberto: como mostrar a autoria (a transação é do Fabio, o `author` do pacote não é verificado), o aceite da licença no envio, a retirada (só do índice, o Arweave não apaga), o limite de pedidos e o critério para pagar a taxa. Detalhes no P160.

**Para além de skills (pedido da Sessão 258, registro)**: agentes e skills seguem o mesmo caminho (pacote, curadores, Arweave, biblioteca, pedidos de publicação); plugins com código vão por **pull request** revisado, com a versão fixada pelo hash do commit. Um agente pede cuidado extra, porque o pacote carrega permissões (tools, autonomia, delegação): a prévia as lista e o agente importado nunca fica acima da autonomia que a configuração de quem instala permite. Uma integração MCP por HTTP é um pacote à parte, com revisão mais forte. O formato `warden-skill` v1 pode virar um envelope genérico com `kind` antes de haver qualquer pacote publicado. Detalhes no P160.

**Formato do pacote (Sessão 258, feito)**: `*.warden-skill.json` com um **envelope** genérico (`format` `warden-package`, `formatVersion` 1, `kind`, `id`, `version`, `author` opcional, `license`, `hash`, `signature` reservada) e o `content` do tipo. Hoje só existe `kind: "skill"` (`content`: `name`, `description`, `body`); um tipo desconhecido é recusado com uma mensagem que diz isso, e agentes e integrações entram depois como outros `kind` no mesmo envelope. O `hash` (`sha256:` de `kind`, `id`, `version`, `author`, `license` e `content`) prova integridade, não autoria. Só texto, no máximo 96 KiB. Exportar e importar por arquivo existem no hub, no desktop e na web.

**Das 8 skills embutidas na 1ª fatia (Sessão 257)**: saem do binário quando as fontes existirem e viram o primeiro conteúdo do repositório do Fabio.

## F. Privacidade e confiança do operador

- **Prometer só o que o modelo atual permite.** O operador consegue, tecnicamente, ver o que o agente vê enquanto trabalha (ARCHITECTURE.md, seção de multiusuário). A cifra por membro (P84) protege backups, discos descartados e leitura casual; **não protege contra quem controla a máquina**. Não prometer "nem nós conseguimos ler".
- **Prometer sim:** acesso restrito e auditado, nenhuma leitura por padrão, nenhum uso dos dados para treino, retenção definida, exportação e apagamento sob demanda.
- **O prompt passa pelo gateway e depois pelo provedor.** Isso entra no texto de privacidade. O gateway registra **só metadados e tokens**, nunca o conteúdo, por padrão.
- **Reforços possíveis:** chave do cofre derivada de uma senha do cliente que não fica em disco (reduz a exposição com a instância parada ou em backup, não enquanto roda destravada). Caminho futuro: o **agente no aparelho do cliente**, com o hub hospedado e a execução sensível no aparelho, pela infraestrutura de nós que já existe (P86/P93).
- **Riscos principais**
  - Token de instância vazado: limitado pelo pré-pago e pelas cotas.
  - Escape de contêiner ou vizinho barulhento: contêiner sem privilégios, cgroups e rede isolada por cliente.
  - **Agente com tools de execução local (`shell`, `code_task`) rodando na máquina do Fabio em nome do cliente:** é o maior risco do hospedado. Pede restrição de saída de rede e revisão do que essas tools podem fazer dentro do contêiner; decidir se começam desligadas (pergunta 8).
  - Chave de provedor do gateway comprometida: cofre de segredos, rotação e orçamento por provedor (o orçamento diário por provedor, `daily_budget_usd`, é da Sessão 285; limita o estrago, não troca a chave).
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
| 0 | Fechar as decisões da seção I, ligar o `MeterSink`, corrigir a espera no `Hello` (P142) e os defeitos de memória da seção K (P145 a P147; a RAM já foi medida) | nada | sim (corrige o self-host também). **Estado (Sessão 225)**: P142, P145, P146 e P147 corrigidos (falta medir o RSS com o binário novo); **faltam** o `MeterSink` e as decisões da seção I |
| 1 | Dockerfile, `--data-dir`, endpoint de saúde, tratamento de SIGTERM, log estruturado (P143), formato de exportação e importação | 0 | sim (ajuda o self-host também). **Feita na Sessão 225**, ver "Estado da fase 1" abaixo |
| 2 | Gateway: token, catálogo de modelos, ledger durável, saldo e `hold`, cotas e rate limit | 0 | sim (testável com a conta criada à mão). **Estado (Sessão 273)**: feito o `crates/warden-gateway` (ledger SQLite em micro-USD, catálogo, reservar → chamar → acertar, endpoint com SSE, tetos de gasto, CLI para criar conta e recarregar à mão); **Sessão 275**: feitos o rate limit por IP e por token e a suspensão por pico de gasto por hora; **Sessão 276**: feito o TLS no próprio gateway (`--tls-cert` e `--tls-key`, certificado relido na renovação); **Sessão 277**: feitos o teto de conexões e de chamadas em andamento, o prazo do stream e o motivo da suspensão no banco; **Sessão 278**: feita a imagem (`Dockerfile.gateway`, smoke com raiz só de leitura no CI, sem publicação); **Sessão 280**: feita a chave do provedor em arquivo de segredo (`api_key_file`, relida quando muda); **faltam** o `MeterSink` na instância e, do cofre, a versão cifrada com chave-mestra (P168) |
| 3 | Contrato B (`warden-hosted-protocol`), recarga idempotente, webhooks de saldo | 2 | com um site mãe falso para teste. **Estado (Sessão 273): o lado do dinheiro está feito** (recarga idempotente por `payment_ref`, saldo, uso por dia, webhooks `credits.low`, `credits.depleted` e `usage.daily_summary`, com um site mãe falso nos testes); **faltam** o B1 em HTTP (provisionamento, fase 4), o B2 (login federado, fase 5) e a loja (B5) |
| 4 | Orquestrador: provisionar, suspender, apagar, backup, upgrade em ondas | 1 e 3 | só tem uso real com o site mãe. **Estado (Sessão 225)**: feitos o núcleo (provisionar, suspender, retomar, apagar, idempotentes por `customer_id`) e a reciclagem, pela CLI do Docker e provados num contêiner de verdade; **Sessão 233**: feito o **backup cifrado por cliente**, com restauração e verificação (ver a seção D), provado com Docker de verdade no CI; **Sessão 234**: feito o **upgrade em ondas** com backup antes e rollback da imagem (ver a seção D), provado no CI; **Sessão 235**: o **backup sai do host** por um comando do operador (`--after-backup`), com a chave à parte (`backup-key`); **faltam** a API HTTP do contrato B1, proxy com TLS, a retenção antes de apagar e o laço como serviço |
| 5 | Login federado do site mãe | 3 | em paralelo com a 4 |
| 6 | Loja curada: `warden-store`, telas, segredos por referência, skills e MCP por HTTP | 1 (segredos) | sim, inclusive no self-host |
| 7 | Endurecimento: auditoria de acesso do operador, chave por cliente, teste de restauração | 4 | aos poucos |

**Adiado de propósito:** multi-tenant num processo, planos, terceiros na loja, agente no aparelho do cliente, embeddings compartilhados.

**Estado da fase 1 (Sessão 225): entregue, com estas provas e estes limites**

| Entrega | Onde | Prova |
|---|---|---|
| Uma pasta para tudo (`--data-dir`, `WARDEN_DATA_DIR`) | `warden_core::paths` (P143 a) | testes de `paths.rs`; a fumaça da imagem grava tudo em `/data` |
| Endpoint de saúde | `GET /health`, público e mínimo (P143 b) | testes de `tests/web_ui.rs`; é o `HEALTHCHECK` da imagem |
| Parada graciosa | SIGTERM e SIGINT com `--shutdown-grace-secs` (P143 c) | `tests/graceful_shutdown.rs`; a fumaça confere saída 0 e `stopped` no log |
| Log estruturado | `tracing`, `--log-format text\|json`, `RUST_LOG`, subscriber em todo binário (P143 d) | testes de `logging.rs`; `serve --log-format json` só emite JSON (fora o `pairing:`, `println!` de propósito) |
| Imagem e raiz só de leitura | `Dockerfile`, `scripts/image-smoke.sh`, `image.yml` (P143 e) | CI verde: `/health`, `/status`, turno com busca, tarefa agendada, webhook, sync por git e SIGTERM com a raiz só de leitura |
| Exportar e importar | `warden-server export` e `import`, pacote em pasta v1 (P141) | 10 testes e a fumaça (export no contêiner, import num volume novo) |
| Publicar a imagem | `image-publish.yml` numa tag `v*` (GHCR) | **publicada** com a tag `v0.1.0` (run 38921092961): `ghcr.io/arven-labs/warden-server:0.1.0`, `:0.1` e `:latest`. **Nasceu privada**: tornar pública é um passo manual de dono da organização (Package settings), ainda por fazer |

Além do que a fase pedia, saíram `GET /status` (a atividade do hub) e a política de reciclagem (`warden-orchestrator`, K.6 item 5), a espera no `Hello` (P142) e a correção do registro de dispositivos (P151).

**O que a fase 1 não prova**: a busca no histórico (o modelo de ~820 MB) e o MCP stdio sob a raiz só de leitura; a medição de RSS do binário novo (P145 a P147); que alguém de fora consiga puxar a imagem (ela foi publicada, mas está privada até o dono da organização torná-la pública); arm64; e o limite de tentativas por IP atrás de um proxy (P142): o hub já limita o `Hello` pelo IP da conexão (Sessão 230), mas atrás do proxy esse IP é o do proxy, então o limite por cliente final fica para o proxy da fase 4.

## I. Perguntas ainda abertas (do Fabio)

> **Sessão 273 (2026-10-22)**: o Fabio deixou claro que **durante um bom tempo isto não será um produto comercial**; o que se constrói aqui é para deixar pronto. E fechou **as perguntas 1, 2, 3 e 9** como as recomendações abaixo (as demais seguem abertas e não travam a fase 2): **1** percentual fixo, guardado por modelo no catálogo (`cost` e `sale` separados) para mudar sem código, e recarga mínima pequena; **2** saldo sem expiração e sem reembolso automático (caso a caso; contrato e imposto só quando houver venda de verdade); **3** saldo zerado bloqueia só os modelos pagos (402 com "recarregue"), a instância segue; **9** um saldo por instância, com o teto por pessoa do `[[limits]]`.

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
12. Existe uma meta de preço que limite o RAM por instância? (Sessão 222: com os consertos da seção K a pergunta deixou de ser "embeddings compartilhados ou não" e virou **qual teto por perfil de plano**: tamanho do cofre indexado, projetos de código ativos e processos MCP.)
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
| 1 | Operar o hub como serviço: imagem, `--data-dir`, health, SIGTERM, log estruturado | P143 | 1 (**feito na Sessão 225**) |
| 2 | Identidade por conta: o desktop só conecta com chave de pareamento (32+ caracteres, e que pareia só o primeiro dispositivo), senha de membro ou TruthID; falta um login federado em que o `sub` da conta vira o dono e emite um token de dispositivo | P138 (seção B2) | 5 |
| 3 | Frontend duplicado entre web e desktop, e o desktop remoto preso ao IPC do Tauri | P144 | a debater |
| 4 | Endpoint público sobre HTTP escrito à mão: sem limite por IP nas mensagens de admin do WebSocket (o `Hello` e os endpoints HTTP `/status`, `/v1` e `/hooks` já têm um por IP da conexão, P142, Sessões 230 e 231), sem teto de conexões, sem CSP nem compressão, sem keep-alive, sem espera para chave errada no `Hello` (P142, **corrigido na Sessão 225**; o limite por IP continua faltando), TLS só por arquivo ou Tailscale. Um proxy reverso (Caddy, Traefik) resolve quase tudo; migrar para hyper ou axum seria grande e não é necessário | P142, P138 | 0 e 1 |
| 5 | Isolamento de execução e de segredos: `shell`, MCP stdio e opencode rodam com o usuário do SO do contêiner, sem sandbox; as chaves de provedor ficam dentro da instância; o operador consegue ver o que o agente vê | P139, P138 | 2 |

**Tradeoffs que ficam em aberto**
- Gap 2: reaproveitar o fluxo do TruthID (menos código, acopla a um emissor) ou OIDC genérico (mais flexível, mais superfície).
- Gap 3: extrair um pacote TypeScript compartilhado com uma interface de transporte (refatoração grande, acaba com a divergência) ou, no desktop remoto, abrir a web UI do hub (barato, mas o webview não tem comandos Tauri, então microfone, cofre local e ferramentas do aparelho não funcionam ali).
- Gap 5: o contêiner por cliente isola um cliente do outro, mas não protege o Fabio do código que o agente executa nem o cliente do Fabio.

**Não verificado nessa revisão:** o comportamento das telas do desktop sem wrapper remoto quando `remote` é verdadeiro; o `warden-cli`, o app móvel e a extensão em profundidade; o fluxo completo de convite de membro e `people::password_gate`; a RAM por instância (medida depois, na seção K); um sistema de arquivos só de leitura de ponta a ponta. As contagens de mensagens do protocolo (~98 do cliente, ~80 do servidor) saíram de uma regex e são aproximadas.

## K. RAM por instância: medições e decisão (Sessão 222)

Pedida pelo Fabio como design review: o maior medo é a RAM por usuário, porque instância cara exige VPS cara e encarece o repasse. Medido primeiro, debatido depois, decidido por último. **Nenhum código do Warden foi alterado**; o que se mediu é o comportamento de hoje, e o conserto proposto foi validado só num benchmark descartável.

### K.1 Como se mediu (e o que isso não cobre)

- **Binário:** `warden-server` em release (82 MB, perfil padrão do cargo) e, para a comparação, o de debug (275 MB). Linux x86_64, **16 núcleos** (limitados com `taskset` a 1, 2 ou 4 quando indicado), 32 GB de RAM, page cache quente.
- **Ambiente de cada execução:** `HOME`, `XDG_CONFIG_HOME` e `XDG_CACHE_HOME` próprios em `/tmp`, um modelo de mentira compatível com OpenAI (SSE) no lugar do provedor, e o servidor dirigido por WebSocket (`hello`, `chat`, `changePassword`). RSS, PSS e CPU lidos de `/proc/<pid>` (`status`, `smaps_rollup`, `stat`).
- **Cofre:** notas sintéticas em português (palavras repetidas ao acaso). "Curta" = 2 a 8 linhas de 4 a 10 palavras; "longa" = 10 a 30 linhas de 8 a 18 palavras. **Não é um cofre real**, e a qualidade da busca não foi avaliada, só a similaridade dos vetores.
- **Benchmark descartável** (fora do repo): um programa Rust com o mesmo `fastembed 6.0.3` e `ort 2.0.0-rc.13` do `Cargo.lock`. Na referência ele reproduziu o servidor: RSS final de 1680 MB contra 1703 MB, 15,5 s contra 15,1 s.
- **Não cobre:** disco frio (sem root para limpar o page cache), início de contêiner e overhead do Docker (não há Docker na máquina), um MCP stdio real (só um processo Node vazio como proxy), nem a CPU de uma VPS de verdade. O binário de release estava em tmpfs.
- **Reproduzir:** os scripts (`measure.py`, `density.py`, e o crate `embedbench`) ficaram só no scratchpad da sessão. Se forem úteis para repetir a medição, o Fabio decide se entram no repo (por exemplo em `tools/`).

### K.2 Números medidos

**Instância ociosa, release:**

| Configuração | RSS | Anônima | PSS |
|---|---|---|---|
| Sem os modelos (`WARDEN_NO_SEMANTIC=1`) | **24,8 MB** | **3,5 MB** | 21 MB |
| Só o modelo do cofre (`AllMiniLML6V2`) | 160 MB | 124 MB | 156 MB |
| Só o modelo do histórico (`ParaphraseMLMiniLML12V2Q`) | 846 MB | 809 MB | 842 MB |
| **Como está hoje (warm-up carrega os dois)** | **960 MB** | 923 MB | 956 MB |

- **100 instâncias enxutas ao mesmo tempo:** RSS de 23 MB cada, mas **PSS de 3,5 MB cada** (346 MB no total), porque as páginas do binário são compartilhadas. 6 instâncias com modelos: 5,7 GB (956 MB cada).
- **Partida:** aceita conexão em **11 ms** (resolução de 10 ms), igual com e sem warm-up, em 5 execuções de cada. O warm-up roda em `spawn_blocking` (`main.rs:1423`), não bloqueia o servidor e gasta ~1,5 s de CPU em segundo plano. O primeiro disparo do binário de debug em disco frio levou 1,7 s (indício, não medida).
- **Modo enxuto em uso:** 30 turnos com um cofre de 300 notas: 24,5 para 29,2 MB, 0,09 s de CPU no total.
- **Processos de fora:** `opencode serve` ocioso, como o hub o inicia: **342 MB** (266 MB anônimos), por projeto de código. Node vazio (proxy de um MCP stdio): 47 MB.

**O 1º turno com cofre indexa tudo de uma vez e o RSS não volta** (`search_semantic_own` chama `model.embed(texts, None)` com todos os blocos pendentes, `memory/mod.rs:516`; o `fastembed` usa lote de 256 e o pico cresce com `lote × bloco²`). Release, 4 núcleos, notas curtas:

| Notas | RSS depois | Acréscimo | 1º turno | CPU |
|---|---|---|---|---|
| 50 | 358 MB | +200 MB | 0,6 s | 2,4 s |
| 300 | 1,29 GB | +1,13 GB | 4,5 s | 17,5 s |
| 1000 | 1,70 GB | +1,55 GB | 15 s | 59 s |
| 3000 | 2,50 GB | +2,34 GB | 45 s | 177 s |

Com **notas longas**, 300 notas chegaram a **8,2 GB** (pico 8,6 GB). O RSS final **não depende dos núcleos** (2,38 GB com 50 notas em 1, 2, 4 ou 16 núcleos); só o tempo muda (7,8 s, 4,6 s, 3,2 s, 3,0 s). Os turnos seguintes são de graça (0,01 s). O histórico tem o mesmo defeito: a 1ª busca depois de 100 turnos soma ~0,87 GB.

**Membros.** O `Vault` de cada membro, com o próprio embedder, é criado **por conexão WebSocket** (`server.rs:1053`) e **por requisição de API** (`openai_api.rs:470`). Medido: o 1º membro eleva o RSS de 157 para 532 MB; os seguintes não somam de forma linear (616 e 598 MB). O custo por requisição de API de membro é inferência do código, **não medido**.

**Benchmark: o que derruba a indexação** (4 núcleos, vetores comparados com a referência pelo cosseno):

| Modelo e corpus | Variante | RSS depois | Tempo | Vetores |
|---|---|---|---|---|
| Cofre, 1000 curtas | hoje (1 lote) | 1685 MB | 15,5 s | referência |
| | **lote 16** | **473 MB** | 10,3 s | idênticos (1,00000) |
| | lote 16 + arena off | 357 MB | 10,6 s | idênticos |
| Cofre, 300 longas | hoje | **7371 MB** | 18,9 s | referência |
| | **lote 16** | 1111 MB | 18,6 s | idênticos |
| | lote 16 + bloco 256 | 409 MB | 6,8 s | cosseno médio 0,917 |
| | lote 16 + bloco 256 + arena off | 224 MB | 6,7 s | cosseno médio 0,917 |
| Histórico, 400 longas (quantizado, lote único forçado) | hoje | **4490 MB** | 42,8 s | referência |
| | **fatias de 16 chamadas** | 1237 MB | 37,1 s | idênticos (1,00000) |
| | fatias de 16 + arena off | 814 MB | 37,3 s | idênticos (1,00000) |
| | fatias 16 + bloco 256 + arena off | 782 MB | 22,5 s | cosseno médio 0,997 |

**Descarregar o modelo** (largar o `TextEmbedding` e medir 2 s depois):
- Cofre, lote 16, arena ligado: **475 MB para 76 MB.** Funciona. Com o arena desligado sobram 276 MB (provável fragmentação do heap).
- Histórico: de 754 MB carregado ficam **~560 MB** depois de largar, com ou sem arena. **Causa desconhecida** (retenção do ORT ou fragmentação). Aqui descarregar no processo não resolve.

### K.3 Capacidade (estimativa derivada dos números)

Premissas **minhas**: 1 GB reservado para SO, orquestrador e proxy, e 15% de folga (utilizável: 2,55 / 5,95 / 12,75 GB em VPS de 4 / 8 / 16 GB). É o pior caso, com todas as instâncias no mesmo estado ao mesmo tempo.

| Perfil por instância | 4 GB | 8 GB | 16 GB |
|---|---|---|---|
| Enxuta, ~30 MB com folga de contêiner (medido: 3,5 MB privados ociosos; o overhead de contêiner é suposição) | ~85 | ~198 | ~425 |
| **Padrão de hoje, ocioso (960 MB)** | **2** | **6** | **13** |
| Hoje, após indexar 300 notas curtas (~2,2 GB, com os dois modelos) | 1 | 2 | 5 |
| Com as correções, usando o cofre (~500 MB de pico, 1000 notas curtas) | 5 | 11 | 25 |
| Com as correções, usando cofre e histórico (~1,3 GB) | 1 | 4 | 9 |
| Com projeto de código ativo, +342 MB cada | somar ao perfil acima | | |

### K.4 O que isso diz sobre as premissas do Fabio

- **O medo está certo, mas o culpado não é o processo.** O hub custa ~3,5 MB privados. O custo está em quatro coisas: o modelo do histórico carregado no warm-up para todos (**820 MB**), o salto da indexação que fica residente (**0,2 a 8 GB**), o `opencode` (**342 MB** por projeto) e os MCP em Node (**47 MB ou mais** cada).
- **Duas dessas coisas são defeitos de código, não limites do produto**, e afetam o self-host também: a indexação em um lote só e o warm-up incondicional.
- **Scale-to-zero (B) e planos por uptime (D) ficam fracos**: ocioso o hub custa ~0 de CPU e ~3,5 MB, então 100 clientes parados cabem em ~350 MB. A diferença de custo está no que se *usa*, não em estar de pé.
- **Multi-tenant num processo (E) não se justifica por RAM**: economizaria ~3,5 MB por cliente contra várias semanas de trabalho e risco de vazamento entre clientes.

### K.5 Decisões (2026-10-15)

| # | Opção | Decisão | Por quê |
|---|---|---|---|
| A | Um contêiner por usuário, orquestrado pela API do site mãe | **Mantida** (já decidida na seção "Decisões") | Barato quando o resto estiver consertado. |
| B | Scale-to-zero do hub | **Descartada.** Hub sempre de pé; só se **recicla** instância ociosa que ficou pesada | Poupa pouco com o hub enxuto e quebra bots (WhatsApp/Telegram), tarefas agendadas, sync e as conexões WebSocket de nós. Reciclar é a única forma comprovada de devolver o modelo do histórico. |
| C | Embedding fora da instância (serviço compartilhado) | **Descartada por ora.** Escolhido: **modelos lazy e locais + consertar a indexação** | O lazy resolve o caso comum com 3 mudanças pequenas e o serviço compartilhado traria fila, ponto único de falha e texto saindo da instância. Fica como plano B se os números dos cenários reais não fecharem. |
| D | Planos por uptime | **Descartada** como alavanca de custo | Always-on é funcionalidade (bots e tarefas), não custo. Limites úteis: tamanho do cofre indexado, projetos de código ativos, processos MCP. |
| E | Multi-tenant num processo | **Descartada** (permanece como dívida da seção G) | Não se paga por RAM. |
| F1 | Consertar a indexação | **Decidida** | Lote 16 no cofre e fatias de 16 no histórico: sem perda de qualidade (cosseno 1,00000) e de 3,5x a 6,6x menos RSS no cofre e 3,6x a 5,5x no histórico. |
| F2 | Execução pesada fora do hub | **Decidida: sidecar sob demanda**, fora da instância | Tira 342 MB por projeto do hub e o risco de execução arbitrária do contêiner do cliente. É o maior esforço novo e depende do orquestrador. |
| F3 | Modelo do histórico só quando alguém usa `search_history` | **Decidida: lazy, sem descarregar**, reciclando a instância ociosa | Descarregar não devolve ~560 MB; um processo auxiliar que sai quando ocioso foi considerado e **adiado** (IPC novo e ~0,7 s de recarga). |
| – | Truncar blocos a 256 tokens | **Descartada por ora** | Muda os vetores dos blocos longos (cosseno 0,92) e exige reindexar e avaliar a busca; reduzir a janela de 40 linhas (`CHUNK_WINDOW_LINES`) fica como estudo à parte, com teste de qualidade. |

**Detalhe técnico que a decisão deixa a confirmar na implementação.** A resposta do Fabio foi "lote 16 + arena". Pelos números, o arena deve ser **ligado no modelo do cofre** (475 MB que caem para 76 MB ao descarregar; desligado, 357 MB mas sobram 276 MB) e **desligado no modelo do histórico** (que nunca é descarregado: 814 a 1237 MB com fatias de 16 (com e sem arena desligado), contra 4490 MB hoje). Descarregar o modelo do cofre após alguns minutos ociosos devolve a memória.

### K.6 Ordem de implementação

Os itens 1 a 5 corrigem o self-host também e não dependem da hospedagem; os pendentes estão em `PENDING.md` (P145 a P148).

1. **P145, indexação do cofre:** `embed(texts, Some(16))` em `search_semantic_own` (`memory/mod.rs:516`), com teste de que os vetores não mudam e de um teto de RSS num cofre grande. **Feito na Sessão 223** (código e teste; rodar o teste e medir de novo ficam para a rodada de testes).
2. **P146, modelos sob demanda:** o warm-up (`main.rs:1423`) vira configurável e fica **desligado no perfil hospedado**; fatias de 16 e arena desligado dentro de `embed::embed` (`memory/embed.rs:46`), para todo chamador de `search_history`; o modelo do cofre é descarregado após ociosidade. **As fatias e o arena desligado foram feitos na Sessão 223**; **o warm-up virou o flag `--no-embedding-warm-up` do `warden-server serve` na Sessão 224** (o perfil hospedado ainda não existe, então quem hospeda passa o flag); **o descarregamento do modelo do cofre também, na Sessão 224** (`--embedding-idle-secs`, padrão 600 s). Falta rodar o teste com modelo e medir o RSS de novo.
3. **P147, `Vault` de membro:** parar de criar um `Vault` (e um embedder) por conexão (`server.rs:1053`) e por requisição de API (`openai_api.rs:470`). **Feito na Sessão 224** (`MemberVaults` em `people.rs`; o descarregamento ocioso passou a cobrir também os vaults dos membros e das pastas compartilhadas). Falta medir o RSS.
4. **P143** (já registrado): `--data-dir`, health, SIGTERM e log estruturado, fase 1 do desenho. **`--data-dir`, `/health` e SIGTERM foram feitos na Sessão 224** e **o log estruturado na Sessão 225** (`--log-format text|json`, `RUST_LOG`); e **a imagem e a raiz só de leitura também na Sessão 225** (`Dockerfile`, `scripts/image-smoke.sh`, workflow `image.yml`; o hub ocioso na imagem usa 23,9 MB, provado só parado). **O formato de exportação e importação saiu na mesma sessão** (`warden-server export` e `import`, pacote em pasta versionado, P141), e com isso **a fase 1 está completa**.
5. **Reciclagem de instância ociosa pesada**, no orquestrador (fase 4). **Feita a parte que não depende de Docker, na Sessão 225**: o hub responde `GET /status` (exige a chave de pareamento) com RSS, conexões, trabalho em andamento e há quanto tempo está parado (`warden_server::activity`, `status.rs`), e o crate `warden-orchestrator` decide *quando* reciclar (`RecyclePolicy`: sem trabalho, RSS ≥ 512 MiB, parado há 15 min, de pé há 1 h). **O laço que consulta e reinicia o contêiner também foi feito na Sessão 225**: `warden-orchestrator recycle [--every N]` pela CLI do Docker, provado com Docker de verdade no CI (`scripts/orchestrator-smoke.sh`), junto com provision, suspend, resume e delete idempotentes. Ver o P138 no `PENDING.md`. Falta o resto da fase 4.
6. **P148, sidecar sob demanda** para `opencode` e MCP stdio, depois do orquestrador.
7. **Medir de novo** com um cofre real, um MCP real e um contêiner real, e fechar os tetos por perfil de plano (pergunta 12 da seção I).

### K.7 O que ficou como suposição ou sem medir

Overhead de contêiner e do orquestrador; disco frio e a rede para baixar os modelos na primeira execução; um MCP stdio real; o custo por requisição de API de membro; a qualidade real da busca (só se mediu a similaridade dos vetores); por que o modelo do histórico retém ~560 MB depois de descarregado; CPU de VPS de verdade; e a hipótese de que o conserto se mantém em cofres reais maiores e mais variados.

### K.8 Medição com o binário novo (Sessão 225)

Repetida no CI (`gh workflow run measure-rss.yml`, run 38968961049, runner Ubuntu 24.04 de 4 núcleos), com o `scripts/measure-rss.py` (versionado, só biblioteca padrão): `warden-server` em release, um hub novo por cenário, modelo de mentira no lugar do provedor, notas sintéticas com semente fixa (mesma definição da K.1), o turno pela API compatível com a OpenAI e o RSS lido de `/proc`, conferido com o `/status`.

| Cenário | RSS depois | Pico | 1º turno | Sessão 222 | Alvo do benchmark |
|---|---:|---:|---:|---:|---:|
| Ocioso, sem modelos | 23 MB | 23 MB | - | 24,8 MB | ~25 MB |
| Ocioso, warm-up dos dois modelos | 880 MB | 894 MB | - | 960 MB | ~960 MB |
| 1º turno, 50 notas curtas | 279 MB | 279 MB | 1,1 s | 358 MB | - |
| 1º turno, 300 notas curtas | 294 MB | 297 MB | 5,7 s | 1290 MB | - |
| 1º turno, 1000 notas curtas | 483 MB | 490 MB | 18,2 s | 1700 MB | ~470 MB |
| 1º turno, 300 notas longas | 419 MB | 419 MB | 10,2 s | 8200 MB | ~1111 MB |

- **P145 confirmado**: 1000 notas curtas caem de 1,7 GB para 483 MB (o alvo era ~470 MB) e 300 longas de 8,2 GB para 419 MB. O pico fica a poucos MB do RSS final, ou seja, o salto do lote único sumiu. Os dois ficaram abaixo do benchmark de lote 16 (1111 MB nas longas), provavelmente porque o ORT e o runner diferem do ambiente da Sessão 222; a causa não foi investigada. O tempo do 1º turno com 1000 notas (18,2 s em 4 núcleos) é parecido com o de antes (15 s): o conserto tirou memória, não tempo.
- **P146 (a) confirmado**: com `--no-embedding-warm-up` a instância ociosa tem 23 MB; com o warm-up, 880 MB.
- **Modelo do histórico (Sessão 256, P156)**: medido à parte, no processo de um teste (`crates/warden-core/tests/history_embed_memory.rs`, `heavy-tests.yml`, runs 39141340193 e 39141804782): 762 MB carregado, 834 MB depois de 400 mensagens longas, **635 MB** ao largar o modelo (a retenção do K.2) e **78 MB** com `malloc_trim(0)` depois (40 MB com `MALLOC_ARENA_MAX=1`). A retenção era o heap do glibc, não o ORT; o `warden-server` agora descarrega os dois modelos por ociosidade e chama `malloc_trim`. O hub inteiro não foi remedido.
- **Não coberto**: o modelo do histórico dentro do hub (não há busca no histórico nesses turnos; o modelo de mentira não pede a tool); cofre real; disco frio e overhead de contêiner. Os alvos de 50 e 300 notas curtas não vinham do benchmark, então a tabela não os compara.

**Segunda rodada (mesmo dia, run 38970343728): descarregamento e membros.** O script ganhou os dois cenários e o `warden-server api-keys create --user <membro>` (P84), que faltava para criar a chave de um membro sem falar WebSocket.

| Cenário | RSS | Pico | Tempo |
|---|---:|---:|---:|
| 1000 notas curtas, depois do 1º turno | 486 MB | 489 MB | 18,7 s |
| … 15 s ocioso (`--embedding-idle-secs 5`) | **113 MB** | - | - |
| … 2º turno (recarrega o modelo) | 177 MB | 489 MB | 0,2 s |

| Membro (50 notas curtas cada) | RSS depois do 1º | 1ª requisição | CPU da 1ª | 5 seguintes (média) | CPU das seguintes |
|---|---:|---:|---:|---:|---:|
| m1 | 245 MB | 1,03 s | 3,60 s | 0,01 s | 0,01 s |
| m2 | 425 MB | 1,04 s | 3,72 s | 0,01 s | 0,01 s |
| m3 | 596 MB | 1,05 s | 3,79 s | 0,01 s | 0,01 s |

- **P146 (c) confirmado**: o modelo do cofre é descarregado (486 para 113 MB; os 76 MB da K.2 eram só o modelo, e o hub sozinho pesa ~24 MB) e o 2º turno o recarrega sem reindexar (0,2 s).
- **P147 confirmado no que ele prometia**: as requisições seguintes de um membro custam 0,01 s de CPU, ou seja, o `Vault` e o modelo vivem entre as requisições. Não há binário antigo para comparar na mesma máquina, então o "antes" continua sendo a inferência do código, não uma medição.
- **Achado novo (P153)**: o RSS cresce **linearmente, cerca de 175 MB por membro ativo** (245, 425 e 596 MB), porque cada membro guarda o seu `Vault` com uma cópia própria do modelo de ~135 MB. A Sessão 222 viu 532, 616 e 598 MB, que não era linear porque o modelo morria com a conexão. Com o cache, o custo fica enquanto o membro estiver ativo e some só depois de `--embedding-idle-secs` (padrão 600) sem busca. Num hub com muitos membros ativos ao mesmo tempo, isso é a RAM que importa.
- **P153 corrigido (Sessão 226)**: o modelo do cofre passou a ser um só por processo (`embed::with_vault_model`), e a indexação o pega por fatia de 16 chunks. A mesma medição (run 38983869046) deu **246, 247 e 275 MB** com 1, 2 e 3 membros (antes 245, 425 e 596): o 3º membro custou 28 MB, e não 170. As requisições seguintes continuam em 0,01 s de CPU, o modelo descarrega (290 para 105 MB) e recarrega (168 MB). A espera de buscas simultâneas foi medida em seguida (cenário "buscas simultâneas" do `measure-rss.py`, run 38985723837, 4 núcleos, 4 membros de 50 notas e um de 1000):
  - **Cofres já indexados**: 0,008 s por requisição em sequência e 0,016 s com os 4 ao mesmo tempo (a pior, 0,022 s). A fila do modelo único é barata no caso comum.
  - **1ª requisição de 4 membros**: 4,44 s no total, tanto em sequência quanto ao mesmo tempo; ao mesmo tempo, todas esperam o lote (a pior 4,44 s, contra 1,22 s). Sem ganho do paralelismo, porque o ORT já usa os núcleos.
  - **Enquanto um membro indexa 1000 notas** (turno de 21,7 s): 5 requisições de membros já indexados levaram **3,9 s em média e 14,1 s na pior**, contra 0,008 s sem indexação. A fatia de 16 chunks **não** deu a vez aos outros como esperado (**P154**). A causa não foi verificada: a hipótese é o `Mutex` da biblioteca padrão não ser justo, mas a CPU do runner também pode ser a limitação.
  - O RSS no fim foi 273 MB (4 membros) e 322 MB (com o membro grande).
- **P154 corrigido (Sessão 226)**: o lock do modelo do cofre passou de `std::sync::Mutex` para `parking_lot::FairMutex` (ordem de chegada). Mesma medição (run 38986614742): durante a indexação de 1000 notas (17,0 s), 58 requisições de outros membros levaram **0,26 s em média e 0,34 s na pior** (antes, 3,9 s e 14,1 s). Com os cofres indexados, 0,014 s por requisição; RSS 274 e 322 MB. A 1ª requisição de 4 membros ao mesmo tempo continua esperando o lote (a pior 3,42 s contra 0,98 s em sequência). Uma rodada de cada, num runner de 4 núcleos. Não separei a CPU como causa à parte: o que se sabe é que trocar o lock bastou. O modelo do histórico tem outro `Mutex` da mesma forma, não medido.

## O que foi verificado e o que não foi

Conferido no código em 2026-10-15: `MeterSink` e `LedgerMeter` não têm chamador fora de `meter.rs`; `ApiKey` tem `raw`, `agent_id` e `user`, sem cota; `SpendEvent` guarda tokens de entrada e saída, provedor e combo; `Provider::Cloud` existe; `FileStore::open` poda por `retain_hours`; `WARDEN_SPEND_LEDGER` existe. Vindo da leitura do código por exploração, **não executado**: os caminhos de arquivo sob `dirs::config_dir()`, a lista de estados globais, a ausência de Dockerfile e de ACME, e a ausência de import/export de skills e agentes. **Medido na Sessão 222** (seção K): RAM por instância, tempo de partida, CPU por turno, `opencode` e um Node vazio. **Ainda não medido:** contêiner, disco frio, um MCP real e o cofre de um usuário real. O `SPEC-lume-desmontagem.md` citado pela spec do router não existe no repositório, e o "site mãe" só aparece na spec e no P131.
