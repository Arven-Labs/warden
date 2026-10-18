# Verificação por código e automação de fluxos

> Origem: vem do Anchor (`anchor/project/AUTOMACAO.md`, Sessão 156 de lá), adotado no Warden na Sessão 261 a pedido da sessão do
> Anchor, que relatou o que o Fabio disse: "prefiro seguir por código, não gosto de fazer essas coisas manualmente". O que ele não quer
> fazer à mão é basicamente **(1) conferir telas** e **(2) disparar passos de um fluxo**. Aqui o processo está adaptado às regras do Warden,
> que valem acima do que o doc de lá diz sobre ferramentas (CI em vez de build local, plan mode a cada item novo, código em inglês,
> editar com Edit/Write). Liga com P129 (nada de teste manual), P149 (CI) e P161 (esta adoção).

## 1. Regras de ouro
1. **Não devolver trabalho ao Fabio.** Nada de fechar uma fatia com "abra o app e confira a tela". Conferir é do Claude, por código; disparar
   passos é do fluxo.
2. **O que é manual por natureza fica dito uma vez, como limite**, não como tarefa nem como opção da lista de próximos passos (veja a seção 5).
3. **Respeitar as regras do Warden**: nada de `cargo`, `flutter` ou teste local (tudo no GitHub Actions, P149); plan mode antes de cada item
   novo; edição de código por Edit/Write para o diff ficar visível; código novo em inglês.

## 2. Parte A: conferir as telas por código
**Ideia**: o E2E de navegador fotografa as telas-chave com `snap(page, "nome")`; o CI sobe as fotos como artefato **sempre** (não só em
falha); o Claude baixa o artefato e **olha cada imagem** (a ferramenta de leitura mostra PNG). Teste de unidade e de fluxo não pega defeito
visual.

### 2.1 Como está montado no Warden
- `web/e2e/harness.mjs` tem o `snap(page, name, { focus })`: grava `<pasta>/<name>.png` (pasta de `WARDEN_E2E_SCREENS`, padrão
  `web/e2e-screens`, no `.gitignore`), rola o elemento `focus` para a vista antes (o que fica abaixo da dobra da janela sai de fora da foto),
  e **nunca derruba o teste**. `signIn(browser, hub, { colorScheme })` abre a página também no tema escuro.
- `web/e2e/screens.test.mjs` percorre as telas principais com dados inventados, em claro e escuro; os outros specs ganham `snap` onde há tela
  cheia.
- `.github/workflows/web-e2e.yml` constrói a página e o hub, instala o Chromium, roda `npm run test:e2e` e sobe `e2e-screens` com
  `if: always()`. Fica fora do `ci.yml` porque compila o hub e instala o navegador.

### 2.2 Como olhar
1. `git push` e esperar o run (`gh run watch <id> --exit-status`).
2. `gh run download <run-id> -n web-e2e-screens -D <pasta nova e vazia do scratchpad>`.
3. Abrir cada PNG com a ferramenta de leitura e **procurar de verdade**: número estranho (`-0,00`, `NaN`, `undefined`), texto que descreve a
   coisa errada ou em idioma errado, jargão interno (nomes de campo, ids), falta de contraste no tema escuro, elemento cortado ou fora da
   dobra. Anotar, corrigir, fotografar de novo e conferir a nova foto.
4. Registrar no `SESSIONS.md` o que a conferência achou e o que foi corrigido.

### 2.3 Cuidados
- **Só dados inventados** nas fotos: o artefato é visível a quem tem acesso ao repositório. Nunca dados reais do Fabio.
- A foto é para o olho; a asserção continua sendo por texto/DOM.
- A web é o que se fotografa hoje. O desktop (Tauri) não tem E2E; as telas só dele (Publicar, QR do Sync) ficam sem conferência até o item do
  P165.

## 3. Parte B: automatizar fluxos
Nove regras, cada uma de algo que deu certo ou errado no Anchor (exemplo guia: estimar sozinho o rendimento do porquinho depois que o
extrato é revisto):
1. **Gatilho pelo estado dos dados, não pelo evento óbvio.** Perguntar "em que momento os dados que o passo precisa já estão completos?".
2. **Melhor esforço**: o passo automático nunca falha a ação do usuário; o erro vai para o log. Um teste cobre isso.
3. **Idempotente**: rodar de novo não duplica, e convive com a fonte exata do dado quando ela chega.
4. **O botão manual continua de reserva**, para forçar e refazer.
5. **Ligar sozinho só com um sinal confiável, e a marca precisa de três estados**: nulo = não decidido, 0 = desligado de propósito, >0 =
   ligado. Com dois estados, desligar era desfeito sozinho.
6. **O resultado passa pela revisão antes de gravar**: o automático cria *propostas*, não grava direto.
7. **Nada de rede em teste.**
8. **Mostrar o estado na tela** (um selo, uma frase com ligar/desligar): ninguém adivinha se está ligado.
9. **Validar o modelo com dados reais antes de construir**, quando o passo estima algo. Se não passar, não vale automatizar.

Nenhum fluxo novo entra sem plan mode e sem o Fabio aprovar. Os candidatos do Warden estão no `PENDING.md` (P162 a P164), todos a debater.

## 4. Ritual de cada fatia
1. Ler o `project/` (INDEX primeiro) e a memória. 2. Plan mode se for item novo. 3. Código e testes, **por Edit/Write**. 4. `git push` e **ler o
CI** (nunca compilar local). 5. Se a fatia tem tela: **baixar e olhar as fotos** (2.2). 6. Registrar no `SESSIONS.md` e no `PENDING.md`: o que foi
feito, como foi verificado, o que não foi.

## 5. O que continua manual por natureza (dito uma vez)
- Dado que só o Fabio tem: uma chave de API (o segredo do repositório), uma senha, uma decisão de gosto.
- Qualquer coisa que precise do celular dele com o TruthID de verdade (publicar no Arweave, o Sync com a carteira): o Claude entrega o fluxo
  testado com o celular de mentira e **diz o limite**.
- Rodar sobre os dados reais dele: o Claude entrega testado com dados inventados.
