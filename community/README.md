# A fonte do Fabio

A primeira fonte de skills do Warden: uma lista assinada de skills prontas, mantida por Fabio Anjos Junior, o autor do Warden. O Warden não traz nenhuma fonte configurada; esta é opcional.

## Adicionar

Em **Skills → Fontes** (na web ou no desktop), use o botão **Adicionar a fonte do Fabio** e salve (num hub, ele pede a chave de pareamento). Ou adicione à mão, com estes dados:

- **Nome**: Fabio
- **Chave pública**: o conteúdo de [`PUBLIC_KEY`](PUBLIC_KEY)
- **Endereço**: `https://raw.githubusercontent.com/Arven-Labs/warden/main/community/index.json`

Depois, **Ver o que ela lista** mostra as skills e **Ler e instalar** abre a prévia: o texto inteiro, a assinatura e o que mudaria. Nada é instalado sem você ler, e nada se atualiza sozinho.

## O que a chave garante, e o que não

- Garante que a lista e cada pacote foram feitos por quem tem a chave privada, e que nada foi alterado no caminho: o índice traz o hash de cada pacote e o Warden confere antes de mostrar.
- Garante que o Fabio pode **retirar** um pacote (o índice tem uma lista de retirados, lida a cada busca) e que um índice mais velho que o já visto é recusado.
- **Não** garante que o texto de uma skill é bom ou seguro para o seu caso: uma skill é instrução que a IA segue. Leia antes de instalar, como em qualquer fonte.

## Como esta pasta é feita

- `skills/*.md`: o texto das skills, no formato do cofre. É a fonte da verdade.
- `packages/*.warden-skill.json`: cada skill empacotada e assinada.
- `index.json`: a lista assinada, com o endereço e o hash de cada pacote e os retirados.
- `PUBLIC_KEY`: a chave pública que assina tudo isso.
- `build.sh`: refaz `packages/` e `index.json` a partir de `skills/`.

Para mudar uma skill: edite o `.md`, rode `community/build.sh ~/.config/warden/curator.key` (a chave privada fica só com o dono, fora do repositório) e envie o `.md`, `packages/` e `index.json` juntos. O número do índice (`sequence`) é o relógio em segundos, então cada índice novo é maior que o anterior. Para retirar um pacote, acrescente `--revoke HASH=motivo` ao `index build` do `build.sh`. O CI não tem a chave: ele só confere que o que está aqui concorda consigo mesmo (`crates/warden-server/tests/community.rs`).
