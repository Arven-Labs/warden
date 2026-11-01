# Arquitetura do Warden: mapa e fluxos

> Conferido contra o código em 2026-11-01, na `main` em `cf42bd6`. Um mapa velho é pior que nenhum: se o código mudou muito desde essa data, desconfie e confira o `arquivo:função` citado.
> Este arquivo diz **por onde as coisas passam hoje**. O **porquê** de cada decisão, sessão a sessão, está em `project/ARCHITECTURE.md`; decisões novas e não óbvias vão para `project/adr/`.

## 1. Mapa de módulos

A regra de dependência é de baixo para cima: `warden-core` não depende de nenhum outro crate do projeto; quase tudo depende dele.

| Crate | O que é | Depende de (crates do projeto) |
|---|---|---|
| `warden-core` | O motor: `Orchestrator` (laço de turno e de tools), provedores de modelo (`model/`), tools (`tool/`), vault e busca (`memory/`), limites e gasto (`budget`, `spend`, `autonomy`). | nenhum |
| `warden-truthid` | Cliente Rust do protocolo `pin()` do TruthID: mostra QR, varre a LAN atrás do celular, cifra a carga. | nenhum |
| `warden-server-protocol` | O protocolo WebSocket + JSON entre hub e clientes (`protocol.rs`: `ClientMessage`/`ServerMessage`) e as peças de cliente (`client.rs`, `discovery.rs`, `tls.rs`). | core, truthid |
| `warden-sync` | Sync do vault e do `config.toml` via Arweave, pago pelo TruthID; tudo cifrado no cliente. `SyncEngine` é a porta de entrada. | core, truthid |
| `warden-bootstrap` | Montagem compartilhada: lê `config.toml`, resolve provedor/modelo/chaves, constrói o `Orchestrator` com as tools, e guarda conversas, agentes, tarefas, org e membros. | core, server-protocol, sync, truthid |
| `warden-server` | O hub (binário `warden-server`): aceita conexões WebSocket, autentica, roda turnos, serve a web UI embutida (`web/dist`) e a API compatível com OpenAI. | bootstrap, core, server-protocol, truthid |
| `warden-cli` | O terminal (binário `warden`): REPL com `ratatui`. | bootstrap, core, sync |
| `warden-telegram`, `warden-whatsapp` | Canais de bot (WhatsApp usa o sidecar Node em `sidecar/whatsapp/`). | bootstrap, core |
| `warden-mcp-server` | O Warden como servidor MCP por stdio: expõe as tools do orquestrador a qualquer cliente MCP (sem porta de rede). | bootstrap, core |
| `warden-mobile-bridge` | Ponte Rust para o app Flutter (`flutter_rust_bridge`). | core, server-protocol, sync |
| `warden-hosted-protocol` | Contrato do Warden hospedado: assinatura HMAC das chamadas (`signature.rs`), créditos, webhooks. | nenhum |
| `warden-gateway` | Gateway de modelo hospedado: cobra créditos pré-pagos por chamada. | hosted-protocol |
| `warden-orchestrator` | Orquestrador de instâncias hospedadas (um contêiner por cliente, via `docker`), com a API do site mãe. | hosted-protocol, server-protocol |

Clientes (fora de `crates/`): `desktop/` (Tauri; o crate `desktop/src-tauri` usa bootstrap, core, server, server-protocol e sync), `web/` (a página servida pelo hub), `mobile/` (Flutter), `extension/` (extensão de navegador, MV3), `site/` (site mãe), `sidecar/whatsapp/` (Baileys).

## 2. Fluxo 1: uma mensagem de chat no terminal ou no desktop

```
Gatilho: a pessoa envia uma mensagem (CLI: Enter no REPL; desktop: botão Enviar)
   ↓
[warden-cli/src/interactive.rs:884]  tokio::spawn chama orchestrator.handle_turn_streaming(history, input, ...)
[desktop/src-tauri/src/lib.rs:335]   o desktop chama o mesmo método
   ↓  (o Orchestrator já vem montado por warden-bootstrap: build_delegating_orchestrator, lib.rs:2573)
[warden-core/src/orchestrator/mod.rs:639] handle_turn_streaming → start_turn → run_turn (mod.rs:710)
   ↓
run_turn monta as mensagens: persona, briefing do projeto, catálogo de skills,
   busca no vault (semântica, com fallback para grep) → entra como contexto
   ↓  laço de até MAX_TOOL_ITERATIONS rodadas:
   (a) spend.gate(): confere os limites de gasto ANTES de cada chamada
   (b) self.model.chat_stream(...)  → ModelProvider (model/openai.rs, anthropic.rs, gemini.rs; fallback.rs para combos)
   (c) registra uso e gasto; sem tool_calls → devolve MessageOutcome e a CLI/desktop desenha a resposta
   (d) com tool_calls → run_tool (mod.rs, após o laço): autonomy::authorize (pode pedir um "sim" humano) → tool.call(args)
       → o resultado volta como mensagem de tool e o laço continua
   ↓
Resultado: texto streamado na tela
```

- **Dado que passa:** `history: &[Message]`, `user_input: &str` e anexos entram; `MessageOutcome { content, usage, tools_used, ... }` sai. Cada pedaço do streaming é um `StreamEvent` entregue ao callback `on_event`.
- **Onde falha:** erro de rede no meio do stream descarta o conteúdo parcial e vira `Err`; estourar o limite de gasto recusa a chamada; passar de `MAX_TOOL_ITERATIONS` rodadas dá erro; uma tool que falha **não** derruba o turno, vira texto `error: ...` para o modelo.
- **Fronteira de confiança:** a resposta do modelo e os argumentos de tool que ele pede são entrada não confiável; quem decide se uma ação perigosa roda é `autonomy::authorize` (aprovação humana), não o modelo.
- **Camada:** tudo no processo do cliente (CLI ou app desktop); só `chat_stream` sai pela rede, para o provedor.
- **Não tracei:** como a conversa é gravada em disco nestes dois caminhos. O desktop grava por `append_messages` (`desktop/src-tauri/src/lib.rs:1026`, num comando à parte do envio); no CLI não achei chamada de gravação em `warden-cli/src/`. No hub, quem grava é o `handle_agent_turn_in_with` (Fluxo 2).

## 3. Fluxo 2: cliente remoto (web, celular, extensão) até o hub

```
Gatilho: a página web / app / extensão envia {"type":"chat","message":...} pelo WebSocket
   ↓
[warden-server/src/server.rs:879]   accept_async: sobe o WebSocket (antes, a web UI olha o cabeçalho HTTP)
   ↓
[server.rs:1006] handle_connection → 1º frame tem de ser Hello (server.rs:1057)
   ↓
Autenticação: attempts.try_attempt(ip) (attempt_limit.rs:54, limita tentativas por IP)
   → keys_match(chave enviada, chave de pareamento) (settings.rs:126) ou device_token já conhecido
   → falhou: reject(...) e fecha; passou: HelloAck
   ↓
[server.rs:1567] ClientMessage::Chat → valida ids (resolve_conversation_id), project/workdir, agent
   ↓  tokio::spawn
[warden-bootstrap/src/lib.rs:1455] handle_agent_turn_in_with → salva a mensagem do usuário na pasta de conversas
   → [lib.rs:1572] turn_orchestrator.handle_turn_streaming(...)   (o mesmo laço do Fluxo 1)
   ↓  cada StreamEvent vira ServerMessage::ChatEvent pelo canal do cliente
Resultado: ChatResponse (ou ChatError) volta pelo WebSocket; a conversa fica no disco do hub
```

- **Dado:** JSON com `type` em camelCase (`ClientMessage` / `ServerMessage` em `warden-server-protocol/src/protocol.rs`). Ex.: `{"type":"chat","message":"oi","conversationId":"c1","agentId":"writer"}`.
- **Onde falha:** chave errada ou tentativas demais → `reject` e conexão fechada; id de conversa inválido → `ChatError` sem rodar nada; modelo com chave inválida → `ChatError` no canal.
- **Fronteira de confiança:** **tudo** que vem do WebSocket (id de conversa, `workdir`, `agent_id`, anexos) é entrada de rede: o id vira nome de arquivo, então é validado antes de qualquer leitura. A chave de pareamento é o segredo do hub.
- **Camada:** servidor (hub). O cliente só desenha.
- **Não tracei:** o caminho de `CodeTurn` (conversas com `workdir`, `server.rs:~1690`), nem o de reconexão e aprovação de tools remotas.

## 4. Fluxo 3: sync do vault (Send e Pull) via TruthID e Arweave

```
Gatilho: a pessoa clica em "Send" (desktop/CLI/celular) no passo 1, depois confirma no celular
   ↓
[warden-sync/src/lib.rs:140]  SyncEngine::begin_push → push::begin_push_with_oauth (push.rs:42)
   diff dos arquivos do vault contra o manifest, monta o bundle e cifra com a vault_key
   → None se nada mudou
   ↓
[lib.rs:148] SyncEngine::finish_push:
   1. sync_lock: um sync por vez
   2. push::ensure_remote_not_newer (push.rs:95): olha as últimas 10 publicações da carteira (Arweave, GraphQL);
      a primeira que abre com esta chave decide pelo contador: maior → "another device sent a newer snapshot";
      igual com outra tx → "diverged". Não conseguir checar também recusa.
   3. push::run_push (push.rs:137): varre a LAN atrás do celular (warden-truthid/lan.rs) e pede o pin() ao TruthID,
      que paga e publica o bundle cifrado no Arweave
   4. salva o manifest novo (contador +1, última tx)
   ↓
Pull: [lib.rs:173] SyncEngine::pull → pull::pull_with_oauth (pull.rs:55): anda pelas publicações mais novas primeiro, pula as
   que não abrem como bundle, decifra, compara o contador; se for mais novo, grava os arquivos e avisa de conflitos locais
Resultado: o vault e o config.toml em outro aparelho ficam iguais ao último Send
```

- **Dado:** `SyncBundle` cifrado (só a `vault_key` abre) e `SyncManifest` local (hash por arquivo, contador, última tx, carteira). A `vault_key` espalha-se para um aparelho novo só pelo pareamento na LAN (`pairing/`), nunca pelo TruthID nem pelo Arweave.
- **Onde falha:** gateway do Arweave fora do ar → o Send é **recusado** (sem a checagem não dá para saber se vai divergir); celular não achado na LAN ou pin recusado → erro, manifest não muda; Pull com contador igual e tx diferente → avisa "diverged" e **não** sobrescreve.
- **Fronteira de confiança:** o que vem do Arweave é dado público e não confiável: só vale depois de decifrar com a `vault_key`. `sync_secrets.json` (a chave em claro) é `0o600`.
- **Janela conhecida:** o Arweave não tem compare-and-swap; outro aparelho ainda pode enviar entre a checagem e a aprovação no celular.
- **Camada:** quem tem o vault (desktop, CLI, celular ou o hub) + o app TruthID no celular + rede Arweave. O hub também sincroniza: `warden-bootstrap/src/auto_sync.rs` (`SyncRunner`, que no Arweave só faz Pull automático, porque o Send precisa do celular) e o `sync_loop` em `server.rs`; a tela Sync da web fala com ele por `warden-server/src/sync.rs`.
- **Não validei:** a criptografia em si (`bundle.rs`, `warden-truthid/crypto.rs`); só descrevo a ordem das chamadas.

## 5. Fluxo 4: Warden hospedado (site mãe → instância → gateway)

```
Gatilho: o cliente assina um plano no site mãe (fora deste repo; contrato em project/HOSPEDAGEM.md, seção B)
   ↓
Site mãe → PUT /v1/instances/{id}   (assinado com HMAC; warden-hosted-protocol/src/signature.rs: sign/verify_any)
   ↓
[warden-orchestrator/src/api.rs:132] handle: verify_any(segredos admin) → 401 se a assinatura ou a data não bate
   → Route::Instance PUT → put (api.rs:~198) → InstanceManager::provision (manager.rs:53)
   → ContainerRuntime (runtime.rs) cria o contêiner Docker do cliente (um por cliente)
   ↓ (suspend / resume / DELETE agendado: api.rs:171-174 → manager.rs:85/96/106; purge-expired apaga os dados depois do prazo)
A instância (um warden-server comum) usa Provider::Cloud e chama o gateway
   ↓
POST /v1/chat/completions   [warden-gateway/src/http.rs:414] com o token da instância como bearer
   ↓
[warden-gateway/src/flow.rs:165] Gateway::reserve: autentica o token, dimensiona o pior caso, bloqueia crédito
   → chama o provedor de modelo (upstream.rs)
   → [flow.rs:230] settle: debita o que a chamada realmente usou e libera a reserva (falha → abandon)
Resultado: a resposta do modelo volta à instância; o saldo do cliente baixa em micro-USD (inteiros, nunca float)
```

- **Dado:** a chamada do site mãe é HTTP assinado (método, path, timestamp e corpo entram na assinatura); o dinheiro é `i64` de micro-USD.
- **Onde falha:** assinatura/relógio errado → 401; segredo admin curto ou com espaço nas pontas → `serve` nem sobe (`validate_secrets`, api.rs); crédito insuficiente → o gateway recusa antes de chamar o provedor; gateway cai no meio → a reserva expira sozinha.
- **Fronteira de confiança:** o site mãe é confiável só por conhecer o segredo admin; o token da instância só conversa com o gateway; o corpo do PUT recusa campos que o contrato prevê mas nada implementa ainda (em vez de ignorá-los).
- **Camada:** servidor do operador (orquestrador + Docker), gateway e provedor de modelo na internet.
- **Ainda não existe** (segundo o `lib.rs` do orquestrador): proxy reverso com TLS e domínio por cliente, e o laço de `serve`/`purge-expired` como serviço. O fluxo acima descreve o que o código faz hoje, não a operação em produção.

## 6. Como manter este arquivo

Mexeu num fluxo acima (renomeou, moveu ou apagou uma função citada)? Atualize o trecho e a data do topo no mesmo PR. Conferência rápida: para cada `arquivo:função` citado, `grep -n 'fn <nome>' <arquivo>` tem de achar.
