# 0001: Mapa de arquitetura à parte e ADRs em `project/adr/`

**Data:** 2026-11-01

## Contexto
O `AGENTS.md` do workspace sugere manter um mapa de módulos e fluxos (§6) e registrar decisões não óbvias em `<projeto>/project/adr/` (§11), e manda perguntar antes de criar. O warden só tinha o `project/ARCHITECTURE.md`: 3.587 linhas, em ordem cronológica por sessão, bom para saber por que algo foi decidido e ruim para achar por onde uma ação passa hoje. Foi o que sobrou do P169 (e), aberto como P177.

## Decisão
- Um mapa curto e à parte em `docs/ARQUITETURA.md`: tabela de crates e 4 fluxos de ponta a ponta, cada salto com `arquivo:função`.
- Decisões novas e não óbvias do warden vão para `project/adr/NNNN-titulo.md` (contexto, decisão, alternativas, consequências), como no workspace e nos outros projetos.
- O `ARCHITECTURE.md` fica como está: continua sendo o histórico por sessão, sem migração.

## Alternativas
- Só o `ARCHITECTURE.md`: um lugar só, mas o mapa não existiria e a decisão ficaria enterrada em 3.500 linhas.
- Migrar o `ARCHITECTURE.md` para ADRs: custo alto, sem ganho para quem só quer a decisão de hoje.

## Consequências
- Há dois lugares para manter em dia. O mapa envelhece quando o código muda: ele leva a data e o commit da última conferência, e mudar um fluxo pede atualizar o trecho no mesmo PR.
- Uma decisão de arquitetura nova passa a render um arquivo pequeno com as alternativas descartadas, e não só um parágrafo no registro.
