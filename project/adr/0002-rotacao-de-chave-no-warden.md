# 0002: Rotação de chave no Warden: época junto da chave, só hub, re-pareamento explícito

**Data:** 2026-11-19

## Contexto
O ADR 0018 da arca (rotação de chave e época desconhecida) decidiu o núcleo (`EpochKeys::next`, `Engine::begin_epoch`, `foreign_epochs`, `discover_epoch`) e deixou ao consumidor o fluxo "perdi um dispositivo" (P-49 da arca, itens 6 e 7). O Warden já tem o aviso de época mais nova (A1, warden#185), mas fixa a época em 0 (`FIRST_KEY_EPOCH`, `warden-sync/src/arca_sync.rs`), não tem onde guardar outra, não gera nem confirma frase BIP-39, não tem credencial S3 e recusa que um dispositivo que já tem chave receba outra (`AlreadyHasKey` em `run_offer`; `adopt_joined_material` não sobrescreve). Quatro escolhas de estrutura precisavam de decisão antes da fatia C do P-49.

Área de chaves (AGENTS.md §4). Decisões do dono em 2026-11-19. Este ADR **só decide**: o código vira PRs próprios, cada um com confirmação.

Fatos do código que moldam o desenho (lidos, não executados):
- O modo arca é de mão única (`arca_mode.rs`): ligado, os motores Arweave e git recusam o cofre. Depois disso a `vault_key` de `sync_secrets.json` tem um único consumidor.
- `sync_secrets.json` é gravado de forma atômica (0600, `save_secrets`) e o pareamento entrega só a `vault_key` (ECIES, ADR 0016 da arca).
- O `take_grant` do hub consome o grant na leitura (P182 i1 do Warden): resposta perdida, grant perdido.

## Decisão
1. **A `vault_key` é sobrescrita no lugar, e só com o modo arca ligado.** Não há uma chave separada para o arca: sem o modo ligado, rotacionar também mudaria a chave dos motores Arweave e git, que não sabem de época.
2. **A época mora em `sync_secrets.json`, ao lado da `vault_key`**, como campo `key_epoch` com `#[serde(default)]` (0), então o arquivo atual continua válido. Chave e época são **um par**: mudam na mesma gravação atômica. Quem recebe a chave (pareamento ou frase) chama `discover_epoch` (arca#177) **uma vez** e grava o resultado; o ciclo normal não descobre nada. `FIRST_KEY_EPOCH` deixa de ser constante do ciclo e vira o valor inicial de um cofre novo.
3. **A credencial S3 fica fora da fatia C.** O Warden não tem S3 ligado (feature `s3` desligada, "frente à parte"). A C cobre: revogar no hub, chave nova, `begin_epoch`, re-parear. Com S3 configurado, a C **recusa** rotacionar. A troca da credencial S3 (ADR 0018 da arca, item 6, passo 2) vira requisito da frente do S3.
4. **Um dispositivo com a chave velha recebe a nova por um caminho "trocar chave" explícito no pareamento**, e não pelo fluxo de dispositivo novo. Regras: só com o modo arca ligado e `foreign_epochs` não vazio (o sinal de que existe época mais nova); as **duas conferências humanas** do ADR 0016 (código digitado antes de conceder, impressão digitada antes de gravar); mantém o `device_id`; grava chave e época juntas (item 2); **zera a réplica local do arca** (`arca-store` e `arca_state.json`, que são cache; a pasta do cofre e as edições locais ficam); e **semeia o estado com o hash que a pessoa confirmou por outro canal** (T15 da arca, limite do P-31 item 1), em vez de aceitar uma vez qualquer manifest de época maior.
5. **Revogar o token do dispositivo perdido, no hub, é uma declaração explícita antes de publicar.** O fluxo mostra o passo, a pessoa declara que já revogou (`warden-server devices revoke`) e só então gera a chave e publica. É um aviso, não uma trava: o cliente não tem como verificar.
6. **Sem recibo no pareamento.** Se a resposta do `take_grant` se perde, o dispositivo que rotacionou ainda tem a chave e concede de novo. O ADR 0016 não muda.

A ordem do fluxo segue o ADR 0018 da arca: (1) revogar o token no hub (declaração), (2) [S3: fora, item 3], (3) gerar a chave nova, mostrar a frase BIP-39 e **a pessoa confirmá-la e guardá-la antes de publicar**, (4) `begin_epoch`, (5) re-parear os demais (item 4).

## Alternativas
- **Chave do arca separada da `vault_key`** (arquivo próprio): descartada. Com o modo arca de mão única não há segundo consumidor para proteger, e duas chaves por cofre seriam um segredo a mais para guardar, parear e rotacionar.
- **Época em `arca_mode.json`:** descartada. É outro arquivo: uma queda entre gravar a chave e gravar a época deixa a chave nova com a época velha, e nada abre. Além disso um `arca_mode.json` ilegível faz todo motor recusar o cofre.
- **Descobrir a época a cada ciclo, sem guardar:** descartada como caminho normal. Lista e baixa todos os manifests (até 64 MiB cada) em todo ciclo. Fica só como ferramenta de recuperação.
- **Esperar a frente do S3, ou construir o cofre e a entrega da credencial S3 dentro da C:** descartadas. A primeira prende a C a uma frente sem data que não é pré-requisito dela. A segunda inventa onde guardar a credencial e estende o pareamento a um segundo segredo: a parte mais cara e mais arriscada do P-49.
- **Apagar o pareamento local e parear de novo como dispositivo novo:** descartada. Perde o `device_id`, obriga a apagar à mão o `sync_secrets.json` e deixa o T15 sem semente de hash.
- **Endpoint de admin no hub para listar e revogar, usado pelo fluxo:** descartada por ora: protocolo novo no `warden-server`, com autenticação de dono, é uma PR e uma área de segurança à parte. Pode vir depois e trocar a declaração por verificação.
- **Recibo no pareamento (fechar o P182 i1):** descartada: muda o ADR 0016 para resolver um caso que a rotação já cobre com a frase confirmada e o dispositivo que concede de novo.

## Consequências
- `SyncSecrets` ganha um campo e o ciclo do arca passa a ler a época dele; é a única mudança de formato (opcional, compatível com o arquivo atual). O struct **não** tem `deny_unknown_fields` (`manifest.rs`, conferido), então um binário antigo lê o arquivo novo sem erro e **ignora** o `key_epoch`: com a chave nova e a época 0 ele não abre nada, e falha fechado (não grava lixo). Não há corrupção, mas a volta ao binário anterior depois da primeira rotação deixa o cofre ilegível para ele.
- A C só aparece com o modo arca ligado e sem S3 configurado; quem usa S3 não tem o fluxo até a frente do S3 trazer o passo 2.
- O passo de revogar vira dependência de uma pessoa que age fora do Warden: se ela declara sem ter revogado, o dispositivo perdido continua escrevendo na época antiga (os de época nova o ignoram, ADR 0018 da arca, item 6, mas ele ainda lê o que a chave velha alcança). O limite fica na tela e na documentação.
- O caminho "trocar chave" muda o contrato do pareamento (hoje só dispositivo novo recebe chave): custo médio de reverter.
- O re-pareamento de um dispositivo ao qual o hub já consumiu o grant exige conceder de novo: um passo manual a mais para quem perde a resposta.

## Não validado
- Conferido no código: há **um** `sync_secrets.json` e **um** `sync_manifest.json` por instalação (`paths.rs`), então o par chave e época é por instalação. Se um dia houver mais de um cofre por instalação, a rotação de um mudaria a chave dos outros.
- O comportamento com um hub real escondendo manifests da época nova; o custo de `begin_epoch` em cofre grande; a frase BIP-39 e o fluxo inteiro num aparelho real (nada disso foi executado).
- A decisão foi levantada e redigida pelo mesmo modelo que leu o código; não houve revisão independente.
